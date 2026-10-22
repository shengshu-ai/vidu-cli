#!/usr/bin/env python3
"""Explicit opt-in, resumable live write regression. Private evidence stays in /tmp.

Default is dry-run. Run --execute --phase baseline, then subsequent phases with
--state-dir. Every named mutation is attempted once, including on resume.
Budget is unlimited per user authorization; quotes are evidence,
not a promise of the final charge. Existing resources are never edited or deleted.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import urllib.request
import urllib.parse
sys.dont_write_bytecode = True
from live_regression import dotenv, redact_strings

ROOT = Path(__file__).resolve().parents[1]

class Runner:
    def __init__(self, args):
        self.args = args
        self.touched=set()
        os.umask(0o077)
        self.path = args.state_dir or Path(tempfile.mkdtemp(prefix='vidu-live-write-', dir='/private/tmp'))
        if not str(self.path.resolve()).startswith('/private/tmp/vidu-live-write-'):
            raise ValueError('state directory must be /private/tmp/vidu-live-write-*')
        self.path.mkdir(mode=0o700, exist_ok=True)
        os.chmod(self.path,0o700)
        self.config = dotenv(args.env_file or ROOT / '.env')
        self.env = dict(os.environ, **self.config)
        self.env.pop('VIDU_DEBUG', None)
        self.binary = ROOT / 'target/release/vidu-cli'
        self.state = json.loads((self.path/'manifest.json').read_text()) if (self.path/'manifest.json').exists() else {'prefix':'cli-regression-20261022-'+self.path.name[-8:], 'budget':None, 'operations':{}, 'tasks':{}, 'resources':{}, 'cases':{}}
        for name,op in self.state['operations'].items():
            f=self.path/(name+'.json')
            if op.get('write') and f.exists() and op['command'][0]=='task' and op['command'][1] in ['submit','rerun','extend','upscale-image','tts','lip-sync']:
                d=json.loads(f.read_text()).get('payload',{}); fields=d.get('error',{}).get('fields',{})
                tid=d.get('task_id') or fields.get('task_id')
                if tid and str(tid) not in self.state['tasks']:
                    self.state['tasks'][str(tid)]={'operation':name,'state':d.get('state') or fields.get('state','pending'),'cleanup':'pending'}
        for name,op in self.state['operations'].items():
            f=self.path/(name+'.json')
            if not op.get('write') or not f.exists():continue
            d=json.loads(f.read_text()).get('payload',{})
            cmd=op.get('command',[])
            if cmd[:2]==['element','create'] and d.get('ok'):
                eid=d.get('id') or d.get('element',{}).get('id')
                if eid:self.state['resources'].setdefault('element',{'id':str(eid),'cleanup':'pending'})
            if cmd[:2]==['voice','create']:
                vid=d.get('voice_id') or d.get('error',{}).get('fields',{}).get('voice_id')
                if vid:self.state['resources'].setdefault('voice',{'id':vid,'cleanup':'retained_no_delete_API'})
        self.save()
    def save(self):
        (self.path/'manifest.json').write_text(json.dumps(redact_strings(self.state,self.config['VIDU_TOKEN']), ensure_ascii=False, indent=2))
        os.chmod(self.path/'manifest.json',0o600)
    def emit(self, **row):
        print(json.dumps(redact_strings(row,self.config['VIDU_TOKEN']),ensure_ascii=False),flush=True)
    def run(self, name, command, write=False, timeout=100):
        self.touched.add(name)
        if name in self.state['operations']:
            file=self.path/(name+'.json')
            if not file.exists():return {}
            cached=redact_strings(json.loads(file.read_text()),self.config['VIDU_TOKEN'])
            file.write_text(json.dumps(cached,ensure_ascii=False,indent=2))
            os.chmod(file,0o600)
            return cached.get('payload',{})
        if write and not self.args.execute:
            self.emit(dry_run=name,command=command)
            return {}
        self.state['operations'][name]={'command':command,'write':write,'started':time.time(),'status':'attempting'}
        self.save() # Durable before POST: timeout or interruption never blindly repeats it.
        try:
            p=subprocess.run([str(self.binary),*command],env=self.env,capture_output=True,text=True,timeout=timeout)
            raw={'exit_code':p.returncode,'stdout':p.stdout,'stderr':p.stderr}
        except subprocess.TimeoutExpired as e:
            raw={'exit_code':None,'stdout':(e.stdout or b'').decode() if isinstance(e.stdout,bytes) else e.stdout or '', 'stderr':'runner_timeout; mutation must not be retried'}
        try: raw['payload']=redact_strings(json.loads(raw['stdout']),self.config['VIDU_TOKEN'])
        except ValueError: raw['payload']={'ok':raw['exit_code']==0,'plain_text':True} if command[:2]==['--format','table'] else {}
        raw=redact_strings(raw,self.config['VIDU_TOKEN'])
        (self.path/(name+'.json')).write_text(json.dumps(raw,ensure_ascii=False,indent=2))
        os.chmod(self.path/(name+'.json'),0o600)
        d=raw['payload'];error=d.get('error',{})
        row={'status':'PASS' if raw['exit_code']==0 and d.get('ok') else 'FAIL','exit_code':raw['exit_code'],'trace_id':error.get('trace_id') or d.get('trace_id'),'error':error}
        self.state['operations'][name].update(row)
        tid=d.get('task_id') or error.get('fields',{}).get('task_id')
        if tid and write and command[:2] in [['task','submit'],['task','rerun'],['task','extend'],['task','upscale-image'],['task','tts'],['task','lip-sync']]:
            self.state['tasks'][str(tid)]={'operation':name,'state':d.get('state','pending'),'cleanup':'pending'}
        self.save()
        self.emit(name=name,status=row['status'],task_id=tid,state=d.get('state'),trace_id=row['trace_id'],error_type=error.get('type'))
        return d
    def get(self,name,path,params):
        self.touched.add(name)
        if name in self.state['operations']:
            file=self.path/(name+'.json')
            d=redact_strings(json.loads(file.read_text()),self.config['VIDU_TOKEN'])
            file.write_text(json.dumps(d,ensure_ascii=False,indent=2))
            os.chmod(file,0o600)
            return d
        u=self.config['VIDU_BASE_URL'].rstrip('/')+path+'?'+urllib.parse.urlencode(params)
        request=urllib.request.Request(u,headers={'Authorization':'Token '+self.config['VIDU_TOKEN']})
        try:
            with urllib.request.urlopen(request,timeout=40) as response: d=json.load(response)
        except urllib.error.HTTPError as e:
            d={'http_status':e.code,'body':e.read().decode()}
        d=redact_strings(d,self.config['VIDU_TOKEN'])
        (self.path/(name+'.json')).write_text(json.dumps(d,ensure_ascii=False,indent=2))
        os.chmod(self.path/(name+'.json'),0o600)
        self.state['operations'][name]={'status':'PASS' if 'http_status' not in d else 'FAIL','write':False}
        self.save();self.emit(name=name,status=self.state['operations'][name]['status'])
        return d
    def poll(self):
        for tid,meta in list(self.state['tasks'].items()):
            if meta['state'] in ['success','failed','canceled']:continue
            name='poll_'+tid+'_'+str(int(time.time()))
            d=self.run(name,['task','get',tid,'--details'])
            meta['state']=d.get('state',meta['state']); self.save()
    def baseline(self):
        self.run('quota_before',['quota','credit'])
        self.run('records_before',['quota','records','--pagesz','100'])
        cost=['task','cost','--type','text2image','--model-version','3.2_image_2_5_fast','--resolution','1080p','--schedule-mode','normal']
        self.run('image_cost',cost)
        self.run('image_submit',['task','submit','--type','text2image','--model-version','3.2_image_2_5_fast','--resolution','1080p','--aspect-ratio','1:1','--schedule-mode','normal','--prompt',self.state['prefix']+' A single friendly red cartoon robot, full body, front view, on a plain white background, simple clean illustration.','--wait','--timeout','1'],True)
        self.run('history_sources',['task','list','--state','success','--pagesz','100'])
    def wait(self):
        self.poll()
    def download(self):
        for tid,meta in self.state['tasks'].items():
            if meta['state']=='success':
                self.run('download_'+tid,['task','get',tid,'--details','--output',str(self.path/('output-'+tid))])
    def reruns(self):
        history=self.run('history_sources',['task','list','--state','success','--pagesz','100'])
        for typ in ['text2image','text2video','img2video','reference2image','character2video']:
            if 'rerun_'+typ in self.state['operations']:continue
            if sum(t['state'] not in ['success','failed','canceled'] for t in self.state['tasks'].values())>=3:break
            source=next((t for t in history.get('tasks',[]) if t.get('type')==typ),None)
            if not source: continue
            # Retain model, transition and media references; override prompt only with a harmless test description.
            settings=source['settings']; model=settings['model_version']
            options=['--prompt',self.state['prefix']+' A friendly robot slowly waves hello.']
            cost=['task','cost','--type',typ,'--model-version',model,'--resolution',settings['resolution'],'--schedule-mode','normal']
            if typ not in ['text2image','reference2image']:
                duration='4' if model in ['viduq3.1-drama-std','3.2_a'] else '3' if model=='3.4' else '2'
                options+=['--duration',duration];cost+=['--duration',duration]
            transition=settings.get('transition')
            if transition and transition!='unspecified':cost+=['--transition',transition]
            quote=self.run('rerun_cost_'+typ,cost)
            if quote.get('ok'):
                self.run('source_details_'+typ,['task','get',source['id'],'--details'])
                self.run('rerun_'+typ,['task','rerun',source['id'],*options],True)
                self.poll()
                # Caller waits between batches; keep at most two active tasks.
                active=sum(t['state'] not in ['success','failed','canceled'] for t in self.state['tasks'].values())
                if active>=3:break
    def derived(self):
        image=next((tid for tid,m in self.state['tasks'].items() if m['operation']=='image_submit' and m['state']=='success'),None)
        if not image:return
        d=self.run('image_details',['task','get',image,'--details'])
        cid=d['creations'][0]['id'];self.state['resources']['image_creation']=cid;self.save()
        self.get('upscale_cost','/vidu/v1/tasks/credits',{'type':'upscale_image','input.creation_id':cid,'settings.resolution':'2k'})
        self.run('upscale_submit',['task','upscale-image','--creation-id',cid,'--resolution','2k'],True)
        hist=json.loads((self.path/'history_sources.json').read_text())['payload']
        source=next((t for t in hist['tasks'] if t['settings']['resolution']=='1080p' and t.get('creations') and t['creations'][0].get('type')=='video'),None)
        if source:
            cid=source['creations'][0]['id']
            self.get('extend_cost','/vidu/v1/tasks/credits',{'type':'video2extend','settings.duration':'1','settings.model_version':'3.1','settings.resolution':'1080p','settings.sample_count':'1','settings.transition':'extendspeed','settings.schedule_mode':'normal'})
            self.run('extend_submit',['task','extend','--creation-id',cid,'--duration','1','--transition','extendspeed'],True)
    def lifecycle(self):
        d=self.run('image_details',['task','get',next(tid for tid,m in self.state['tasks'].items() if m['operation']=='image_submit'),'--details'])
        uri=d['creations'][0]['uri']
        local=next(self.path.glob('output-'+d['task_id']+'/*.png'),None)
        initial=self.path/'element_create.json'
        if initial.exists():
            first=json.loads(initial.read_text()).get('payload',{})
            error=first.get('error',{})
            safe_local_retry=error.get('type')=='client_error' and error.get('message','').startswith('Cannot read image:')
            if first.get('ok') or not safe_local_retry:
                el=first # Never create again after success or ambiguous transport error.
            else:
                el=self.run('element_create_local',['element','create','--name',self.state['prefix'],'--image',str(local) if local else uri],True)
        else:
            el=self.run('element_create',['element','create','--name',self.state['prefix'],'--image',str(local) if local else uri],True)
        eid=el.get('id') or el.get('element',{}).get('id')
        if eid:
            self.state['resources']['element']={'id':str(eid),'cleanup':'pending'};self.save()
            self.run('element_update_name',['element','update',str(eid),'--name',self.state['prefix']+'-updated'],True)
            self.run('element_update_recaption',['element','update',str(eid),'--recaption','A friendly red cartoon robot on a white background.'],True)
            self.run('element_clear_labels',['element','update',str(eid),'--clear-labels'],True)
            self.run('element_updated_get',['element','get',str(eid)])
        cid=self.state['resources']['image_creation']
        before=self.run('creation_favorites_before',['creation','list','--favorites-only','--pagesz','100'])
        if not before.get('ok'):raise RuntimeError('creation favorite snapshot unavailable')
        if str(cid) in [str(c['id']) for c in before.get('creations',[])]:raise RuntimeError('new creation unexpectedly already favorited; preserve it')
        self.run('creation_favorite',['creation','favorite',cid],True)
        self.run('creation_favorites_during',['creation','list','--favorites-only','--pagesz','100'])
        self.run('creation_unfavorite',['creation','unfavorite',cid],True)
        self.run('creation_favorites_after',['creation','list','--favorites-only','--pagesz','100'])
        # Complete cursor traversal before mutating a shared favorite.
        snapshot=[]; token='';seen=set()
        while token not in seen:
            seen.add(token)
            page=self.run('element_favorites_before_'+str(len(seen)),['element','favorites','--pagesz','100','--page-token',token])
            if not page.get('ok'):raise RuntimeError('favorite snapshot incomplete; no favorite mutation authorized')
            snapshot+=page.get('elements',[]);token=page.get('next_page_token')
            if not token:break
        self.state['resources']['favorite_snapshot_complete']=True
        self.state['resources']['favorite_snapshot_ids']=[str(e['id']) for e in snapshot];self.save()
        search=self.run('community_candidates_cat',['element','search','--keyword','猫','--pagesz','20'])
        pick=next((e for e in search.get('elements',[]) if str(e['id']) not in self.state['resources']['favorite_snapshot_ids']),None)
        if pick:
            eid=str(pick['id']);self.state['resources']['community_favorite']=eid;self.save()
            self.run('community_favorite',['element','favorite',eid],True)
            self.run('community_favorites_during',['element','favorites','--pagesz','100'])
            self.run('community_unfavorite',['element','unfavorite',eid],True)
            self.run('community_favorites_after',['element','favorites','--pagesz','100'])
    def cancel(self):
        self.run('cancel_cost_1080',['task','cost','--type','text2video','--model-version','3.2','--duration','4','--resolution','1080p','--transition','speed','--schedule-mode','normal'])
        d=self.run('cancel_submit_1080',['task','submit','--type','text2video','--model-version','3.2','--duration','4','--resolution','1080p','--transition','speed','--schedule-mode','normal','--prompt',self.state['prefix']+' A red robot slowly waves.'],True)
        if d.get('task_id'):
            self.run('cancel_new_task',['task','cancel',d['task_id']],True)
            self.poll()
    def headtail(self):
        history=self.run('headtail_history',['task','list','--type','headtailimg2video','--state','success','--pagesz','100'])
        if history.get('tasks'):
            source=next(t for t in history['tasks'] if t['settings']['model_version'] in ['3.0','3.1','3.2','3.2_a']);settings=source['settings'];model=settings['model_version']
            duration='2' if model in ['3.1','3.2'] else '4' if model=='3.2_a' else '5'
            cost=['task','cost','--type','headtailimg2video','--model-version',model,'--duration',duration,'--resolution','1080p','--schedule-mode','normal']
            if settings.get('transition') not in [None,'unspecified']:cost+=['--transition',settings['transition']]
            self.run('headtail_rerun_cost',cost)
            self.run('source_details_headtailimg2video',['task','get',source['id'],'--details'])
            self.run('rerun_headtailimg2video',['task','rerun',source['id'],'--duration',duration,'--prompt',self.state['prefix']+' A robot gently waves.'],True)
            return
        self.state['cases']['F10_headtailimg2video']={'status':'BLOCKED','reason':'no_supported_successful_owned_source; no_upload_retry'}
        self.save()
    def voice(self):
        self.get('voice_cost','/credit/v1/credits/cost',{'biz_code':'material/voice'})
        self.get('voice_trials','/credit/v1/trials/me/all',{})
        audio=self.path/'voice-source.wav'
        if not audio.exists():
            text='This is a generic synthetic test recording for software validation. A friendly robot is standing in a quiet garden. The morning sunlight is warm and the sky is clear. We are checking that the speech is easy to hear, calm, steady, and free of background noise. This recording does not imitate any real person.'
            subprocess.run(['/usr/bin/say','-v','Samantha','-r','150','-o',str(self.path/'voice-source.aiff'),text],check=True)
            subprocess.run(['/usr/bin/afconvert','-f','WAVE','-d','LEI16',str(self.path/'voice-source.aiff'),str(audio)],check=True)
        d=self.run('voice_create',['voice','create','--audio',str(audio),'--name',self.state['prefix'],'--wait','--timeout','30'],True,timeout=180)
        vid=d.get('voice_id') or d.get('error',{}).get('fields',{}).get('voice_id')
        if vid:
            self.state['resources']['voice']={'id':vid,'cleanup':'retained_no_delete_API'};self.save()
            self.run('voice_update',['voice','update',vid,'--name',self.state['prefix']+'-updated','--description','Generic synthetic software regression test voice.'],True)
            self.run('voice_list',['voice','list','--pagesz','100'])
            self.run('voice_get',['voice','get',vid])
            self.run('voice_wait',['voice','wait',vid,'--timeout','30'])
            self.run('personal_tts_cost',['task','tts-cost','--voice-id',vid,'--text','你好，这是软件回归测试的通用朗读。','--schedule-mode','normal'])
            self.run('personal_tts',['task','tts','--voice-id',vid,'--prompt','你好，这是软件回归测试的通用朗读。','--schedule-mode','normal'],True)
    def summary(self):
        self.poll()
        self.audit()
        def ids(d,key):return sorted(str(x['id']) for x in d.get(key,[]))
        def payload(n):
            f=self.path/(n+'.json')
            return json.loads(f.read_text()).get('payload',{}) if f.exists() else {}
        cases=self.state['cases']
        image=next((m for m in self.state['tasks'].values() if m['operation']=='image_submit'),{})
        image_tid=next((tid for tid,m in self.state['tasks'].items() if m['operation']=='image_submit'),None)
        files=list(self.path.glob('output-'+image_tid+'/*.png')) if image_tid else []
        cases['A_basic_image']={'status':'PASS' if image.get('state')=='success' and any(f.stat().st_size for f in files) else 'FAIL','download_bytes':sum(f.stat().st_size for f in files)}
        for case,op in [('F08_extend','extend_submit'),('F09_upscale','upscale_submit')]:
            task=next((m for m in self.state['tasks'].values() if m['operation']==op),{})
            cases[case]={'status':'PASS' if task.get('state')=='success' else 'FAIL' if task.get('state')=='failed' else 'PENDING','state':task.get('state')}
        for case,pre,during,post,key in [('F11_creation_favorite','creation_favorites_before','creation_favorites_during','creation_favorites_after','creations'),('F06_community_favorite','element_favorites_before_1','community_favorites_during','community_favorites_after','elements')]:
            target=self.state['resources'].get('image_creation') if key=='creations' else self.state['resources'].get('community_favorite')
            before=ids(payload(pre),key);after=ids(payload(post),key);middle=ids(payload(during),key)
            cases[case]={'status':'PASS' if target and str(target) in middle and before==after else 'FAIL','restored':before==after,'before_count':len(before),'after_count':len(after)}
        canceled=next((m for m in self.state['tasks'].values() if m['operation']=='cancel_submit_1080'),{})
        cases['F02_cancel']={'status':'PASS' if canceled.get('state')=='canceled' else 'FAIL','state':canceled.get('state')}
        cases['F03_cleanup']={'status':'PASS' if self.state['tasks'] and all(m.get('cleanup')=='deleted' and m.get('history_absent') is True for m in self.state['tasks'].values()) and (not self.state['resources'].get('element') or self.state['resources']['element'].get('cleanup')=='deleted') else 'PENDING','deleted':sum(m.get('cleanup')=='deleted' for m in self.state['tasks'].values()),'created':len(self.state['tasks'])}
        cases['F04_F05_personal_element']={'status':'BLOCKED' if not self.state['resources'].get('element') else 'PASS' if all(self.state['operations'].get(n,{}).get('status')=='PASS' for n in ['element_update_name','element_update_recaption','element_clear_labels','element_updated_get','element_delete']) else 'FAIL','reason':'development_upload_host_TLS_EOF; no_element_created; no_retry'}
        cases['F12_voice_clone_personal_tts_lipsync']={'status':'BLOCKED','reason':'voice_recording_upload_TLS_EOF; no_voice_id; dependent_personal_tts_and_lipsync_not_submitted','upload_id':None}
        retained_voice=self.state['resources'].get('voice')
        if retained_voice:
            personal=next((m for m in self.state['tasks'].values() if m['operation']=='personal_tts'),{})
            lipsync=next((m for m in self.state['tasks'].values() if m['operation']=='personal_lipsync'),{})
            cases['F12_voice_clone_personal_tts_lipsync']={'status':'PASS' if personal.get('state')=='success' and lipsync.get('state')=='success' else 'PENDING' if personal or lipsync else 'BLOCKED','reason':'personal_voice_recorded; dependent_generation_results_checked_separately','voice_id_recorded':True,'personal_tts_state':personal.get('state'),'lipsync_state':lipsync.get('state')}
        cases['F05_label_replace']={'status':'BLOCKED','reason':'no_new_element_due_upload_TLS; no_verified_owned_label_id'}
        downloads={}
        for tid,meta in self.state['tasks'].items():
            op=self.state['operations'].get('download_'+tid)
            if op:
                downloads[tid]={'status':op['status'],'generation_state':meta['state'],'bytes':sum(f.stat().st_size for f in (self.path/('output-'+tid)).glob('*') if f.is_file()),'error_type':op.get('error',{}).get('type')}
        self.state['downloads']=downloads
        official=next(((tid,m) for tid,m in self.state['tasks'].items() if m['operation']=='official_tts_submit'),None)
        if official:
            audio_files=[f for f in (self.path/('output-'+official[0])).glob('*') if f.is_file()]
            cases['official_TTS']={'status':'PASS' if official[1]['state']=='success' and any(f.suffix.lower() in ['.mp3','.wav','.m4a','.aac'] and f.stat().st_size>0 for f in audio_files) else 'FAIL','state':official[1]['state'],'files':[{ 'extension':f.suffix,'bytes':f.stat().st_size} for f in audio_files],'audio_validation':json.loads((self.path/'official_tts_media_validation.json').read_text()).get('exit_code')==0 if (self.path/'official_tts_media_validation.json').exists() else None}
        repaired=next((m for m in self.state['tasks'].values() if m['operation']=='rerun_text2image_verified_model'),None)
        if repaired:cases['F10_text2image_verified_model']={'status':'PASS' if repaired['state']=='success' else 'FAIL' if repaired['state']=='failed' else 'PENDING','state':repaired['state']}
        residuals=[]
        for name,op in self.state['operations'].items():
            error=payload(name).get('error',{});step=error.get('step','')
            if op.get('write') and error.get('type')=='network_error' and (step=='put_image' or step.startswith('voice_upload:')):
                residuals.append({'operation':name,'kind':'voice' if step.startswith('voice_upload:') else 'image','upload_id':step.split(':',1)[1] if ':' in step else None,'reason':'create_upload succeeded; signed PUT failed; no retry'})
        self.state['resources']['upload_residuals']={'count':len(residuals),'records':residuals}
        voice_residual=next((r for r in residuals if r['kind']=='voice'),{})
        cases['F12_voice_clone_personal_tts_lipsync']['upload_id']=voice_residual.get('upload_id')
        expected=payload('image_submit').get('error',{})
        cases['E01_submit_wait_json_stderr_download']={'status':'PASS' if expected.get('type')=='timeout_error' and expected.get('fields',{}).get('task_id') and files else 'FAIL','resumed_same_task':True,'download_bytes':sum(f.stat().st_size for f in files)}
        cases['E02_table']={'status':'PASS' if self.run('quota_table',['--format','table','quota','credit']).get('ok') else 'FAIL'}
        self.run('quota_after_'+str(int(time.time())),['quota','credit'])
        self.run('records_after_'+str(int(time.time())),['quota','records','--pagesz','100'])
        self.save();self.emit(cases=cases)
    def cleanup(self):
        self.poll()
        resources=self.state['resources']
        community=resources.get('community_favorite')
        if community and community not in resources.get('favorite_snapshot_ids',[]) and resources.get('favorite_snapshot_complete') and self.state['operations'].get('community_favorite',{}).get('status')=='PASS':
            self.run('community_unfavorite',['element','unfavorite',community],True)
        cid=resources.get('image_creation')
        before_file=self.path/'creation_favorites_before.json'
        before=json.loads(before_file.read_text()).get('payload',{}) if before_file.exists() else {}
        if cid and before.get('ok') and str(cid) not in [str(c['id']) for c in before.get('creations',[])] and self.state['operations'].get('creation_favorite',{}).get('status')=='PASS':
            self.run('creation_unfavorite',['creation','unfavorite',cid],True)
        for tid,m in self.state['tasks'].items():
            if m['state'] in ['success','failed','canceled'] and m.get('cleanup')!='deleted':
                d=self.run('delete_'+tid,['task','delete',tid],True)
                if d.get('ok'):m['cleanup']='deleted';self.save()
        e=self.state['resources'].get('element')
        if e and e.get('cleanup')!='deleted':
            d=self.run('element_delete',['element','delete',e['id']],True)
            if d.get('ok'):e['cleanup']='deleted';self.save()
        history=self.run('history_after_cleanup_'+str(int(time.time())),['task','list','--pagesz','100'])
        visible={str(t['id']) for t in history.get('tasks',[])}
        for tid,m in self.state['tasks'].items():
            if m.get('cleanup')=='deleted':m['history_absent']=history.get('ok') is True and tid not in visible
        self.save()
        self.run('elements_after_cleanup',['element','list','--pagesz','100'])
    def image_rerun(self):
        tid=next(t for t,m in self.state['tasks'].items() if m['operation']=='image_submit')
        self.run('image_rerun_cost',['task','cost','--type','text2image','--model-version','3.2_image_2_5_fast','--resolution','1080p','--schedule-mode','normal'])
        self.run('rerun_text2image_verified_model',['task','rerun',tid,'--prompt',self.state['prefix']+' A single friendly red cartoon robot, full body, front view, white background.','--wait','--timeout','1'],True)
    @staticmethod
    def media_identity(d):
        result=[]
        for prompt in d.get('input',{}).get('prompts',[]):
            if prompt.get('type')=='text':continue
            identity={k:prompt[k] for k in ['type','content','name','material','lip_sync'] if k in prompt}
            content=identity.get('content','')
            if isinstance(content,str) and content.startswith(('https://','http://')):
                # Expiring signatures are not asset identity; retain host and complete object path.
                parsed=urllib.parse.urlsplit(content)
                identity['content']=urllib.parse.urlunsplit((parsed.scheme,parsed.netloc,parsed.path,'',''))
            result.append(identity)
        return result
    def media_equivalent(self,old,new):
        a=self.media_identity(old);b=self.media_identity(new)
        if len(a)!=len(b):return False
        originals=[x for x in old.get('input',{}).get('prompts',[]) if x.get('type')!='text']
        copies=[x for x in new.get('input',{}).get('prompts',[]) if x.get('type')!='text']
        hashes=self.state.setdefault('input_hashes',{})
        def digest(url):
            if url in hashes:return hashes[url]
            if not url.startswith('https://'):return None
            try:
                # Signed source/copy URLs only; never send account Authorization to storage.
                with urllib.request.urlopen(url,timeout=30) as reply:
                    data=reply.read(30*1024*1024+1)
                if len(data)>30*1024*1024:return None
                hashes[url]=hashlib.sha256(data).hexdigest();self.save()
                return hashes[url]
            except Exception:return None
        for x,y,original,copy in zip(a,b,originals,copies):
            if x==y:continue
            xc=x.pop('content',None);yc=y.pop('content',None)
            if x!=y:return False
            if xc!=yc:
                h1=digest(original.get('content',''));h2=digest(copy.get('content',''))
                if not h1 or not h2:return None
                if h1!=h2:return False
        return True
    def audit(self):
        history=json.loads((self.path/'history_sources.json').read_text())['payload']
        for typ in ['text2image','text2video','img2video','headtailimg2video','reference2image','character2video']:
            if 'rerun_'+typ not in self.state['operations']:continue
            sources=history['tasks'] if typ!='headtailimg2video' else json.loads((self.path/'headtail_history.json').read_text())['payload']['tasks']
            source=next(t for t in sources if t['type']==typ and (typ!='headtailimg2video' or t['settings']['model_version'] in ['3.0','3.1','3.2','3.2_a']))
            old=self.run('source_details_'+typ,['task','get',source['id'],'--details'])
            tid=next(t for t,m in self.state['tasks'].items() if m['operation']=='rerun_'+typ)
            if self.state['tasks'][tid].get('cleanup')=='deleted':
                evidence=sorted(self.path.glob('audit_'+typ+'_*'))
                new=json.loads(evidence[-1].read_text()).get('payload',{}) if evidence else {}
            else:
                new=self.run('audit_'+typ+'_'+str(int(time.time())),['task','get',tid,'--details'])
                if new.get('state'):self.state['tasks'][tid]['state']=new['state']
            equivalent=self.media_equivalent(old,new)
            model_preserved=old.get('settings',{}).get('model_version')==new.get('settings',{}).get('model_version')
            generation_status='PASS' if new.get('state')=='success' else 'FAIL' if new.get('state')=='failed' else 'PENDING'
            status=generation_status
            if generation_status=='PASS':
                status='PASS' if equivalent is True and model_preserved else 'BLOCKED' if equivalent is None and model_preserved else 'FAIL'
            self.state['cases']['F10_'+typ]={'status':status,'generation_status':generation_status,'state':new.get('state'),'reference_identity_preserved':equivalent,'model_preserved':model_preserved}
        self.save()
    def official_tts(self):
        voices=self.run('official_tts_voices',['task','tts-voices'])
        # Stable official voice used by CLI examples; no personal voice dependency.
        vid='English_Aussie_Bloke'
        self.run('official_tts_cost',['task','tts-cost','--voice-id',vid,'--text','Hello, this is a clear software regression test.','--schedule-mode','normal'])
        self.run('official_tts_submit',['task','tts','--voice-id',vid,'--prompt','Hello, this is a clear software regression test.','--schedule-mode','normal','--wait','--timeout','1'],True)
    def charges(self):
        stamp=str(int(time.time()));rows=[]
        start=min(op.get('started',time.time()) for op in self.state['operations'].values())
        for page in range(10):
            d=self.run('ledger_'+stamp+'_'+str(page),['quota','records','--page',str(page),'--pagesz','100'])
            current=d.get('records',[]);rows+=current
            if len(current)<100:break
            from datetime import datetime
            times=[datetime.fromisoformat(r['created_at'].replace('Z','+00:00')).timestamp() for r in current if r.get('created_at')]
            if times and min(times)<start:break
        charges={}
        for tid in self.state['tasks']:
            matches=[r for r in rows if str(r.get('task_id'))==tid]
            charges[tid]={'debits':-sum(min(r.get('amount',0),0) for r in matches),'refunds':sum(max(r.get('amount',0),0) for r in matches),'net_spent':-sum(r.get('amount',0) for r in matches),'record_count':len(matches)}
        self.state['charges']={'by_task':charges,'net_spent':sum(x['net_spent'] for x in charges.values()),'scope':'only exact task_id ledger matches; uploads without task_id not attributed','snapshot_time':time.time()}
        self.run('quota_final_'+stamp,['quota','credit'])
        self.save();self.emit(charges=self.state['charges'])
    def dispatch(self):
        previous=set(self.state['operations'])
        getattr(self,self.args.phase)()
        self.emit(state_dir=str(self.path),tasks=self.state['tasks'])
        failed=[]
        for name in self.touched:
            op=self.state['operations'][name]
            expected_timeout=name in ['image_submit','rerun_text2image_verified_model','official_tts_submit'] and op.get('error',{}).get('type')=='timeout_error' and bool(op.get('error',{}).get('fields',{}).get('task_id'))
            if op['status']=='attempting' or (op['status']=='FAIL' and not expected_timeout):failed.append(name)
        if self.args.phase in ['summary','audit'] and any(c.get('status') in ['FAIL','BLOCKED','PENDING'] for c in self.state['cases'].values()):failed.append('incomplete_cases')
        if self.args.phase=='wait' and any(m['state']=='failed' for m in self.state['tasks'].values()):failed.append('failed_task')
        if self.args.phase in ['wait','cleanup'] and any(m['state'] not in ['success','failed','canceled'] for m in self.state['tasks'].values()):failed.append('pending_task')
        if failed:raise SystemExit(1)

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--execute',action='store_true')
    p.add_argument('--state-dir',type=Path)
    p.add_argument('--env-file',type=Path)
    p.add_argument('--phase',choices=['baseline','wait','download','reruns','derived','lifecycle','cancel','headtail','voice','summary','cleanup','image_rerun','audit','official_tts','charges'],default='baseline')
    a=p.parse_args()
    if not a.execute:
        print('Dry-run: no network or mutation. Use --execute to run phases; budget default is unlimited.')
        return
    Runner(a).dispatch()
if __name__=='__main__':main()
