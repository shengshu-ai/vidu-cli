#!/usr/bin/env python3
"""Production-only opt-in rerun of failed/blocked live cases.

No writes without --execute. Resume requires the identical HTTPS service endpoint
and token fingerprint. Evidence is private under /private/tmp/vidu-live-write-prod-*.
Every named mutation is durable before execution and is never blindly repeated.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
sys.dont_write_bytecode = True
from live_write_regression import Runner, ROOT
from live_regression import dotenv, redact_strings

BASE = 'https://service.vidu.cn'

class ProductionRunner(Runner):
    def __init__(self,args):
        config=dotenv(args.env_file)
        actual=config.get('VIDU_BASE_URL','').rstrip('/')
        if actual!=BASE or not config.get('VIDU_TOKEN'):
            raise ValueError('production env must contain token and exact https://service.vidu.cn endpoint')
        binding={'base_url':actual,'token_sha256':hashlib.sha256(config['VIDU_TOKEN'].encode()).hexdigest()}
        os.umask(0o077)
        if args.state_dir is None:
            args.state_dir=Path(tempfile.mkdtemp(prefix='vidu-live-write-prod-',dir='/private/tmp'))
        resolved=args.state_dir.resolve()
        if resolved.parent!=Path('/private/tmp') or not resolved.name.startswith('vidu-live-write-prod-'):
            raise ValueError('invalid production evidence directory')
        mf=resolved/'manifest.json'
        if mf.exists() and json.loads(mf.read_text()).get('environment_binding')!=binding:
            raise ValueError('resume environment/token fingerprint mismatch; no operation permitted')
        if not mf.exists():
            resolved.mkdir(mode=0o700,exist_ok=True)
            mf.write_text(json.dumps({'prefix':'cli-regression-20261022-prod-'+resolved.name[-8:],'environment_binding':binding,'operations':{},'tasks':{},'resources':{},'cases':{},'budget':None}))
            mf.chmod(0o600)
        super().__init__(args)
        self.preflight=args.preflight_dir
        if self.preflight:self.state['preflight_dir']=str(self.preflight)
        elif self.state.get('preflight_dir'):self.preflight=Path(self.state['preflight_dir'])
        label_file=self.path/'label_create.json'
        if label_file.exists():
            label=json.loads(label_file.read_text()).get('payload',{})
            if label.get('id') and self.state['operations'].get('label_create',{}).get('write') is True and self.state['operations']['label_create'].get('command',[None,None])[1]=='POST':
                self.state['resources'].setdefault('label',{'id':str(label['id']),'cleanup':'pending'})
        self.save()
    def active(self):
        return sum(m['state'] not in ['success','failed','canceled'] for m in self.state['tasks'].values())
    def submit(self,name,cmd):
        if name not in self.state['operations'] and self.active()>=3:
            self.emit(name=name,status='DEFERRED',reason='three_active_tasks')
            return {}
        return self.run(name,cmd,True,timeout=180)
    def load_preflight(self,name):
        if not self.preflight:return {}
        f=self.preflight/(name+'.json')
        if not f.exists():return {}
        raw=json.loads(f.read_text())
        return raw.get('payload') or json.loads(raw['stdout']) if 'stdout' in raw else raw
    def setup(self):
        self.prepare_fixtures('image', 'audio')
        self.run('quota_before',['quota','credit'])
        self.run('records_before',['quota','records','--pagesz','100'])
        self.run('production_history',['task','list','--state','success','--pagesz','100'])
        self.run('production_headtail_history',['task','list','--state','success','--type','headtailimg2video','--pagesz','100'])
        self.run('production_elements',['element','list','--pagesz','100'])
        self.run('production_voices',['voice','list','--pagesz','100'])
    def prepare_fixtures(self, *kinds):
        """Validate all inputs before API calls; keep private copies on resume."""
        names={'image':'robot.png','audio':'generic-synthetic-voice.wav'}
        pending=[]
        for kind in kinds:
            dest=self.path/names[kind]
            if dest.is_symlink() or (dest.exists() and not dest.is_file()):
                raise ValueError(f'private {kind} fixture must be a regular file')
            if dest.is_file():
                dest.chmod(0o600)
                continue
            source=getattr(self.args,kind+'_fixture',None)
            if source is None or source.is_symlink() or not source.is_file():
                raise ValueError(f'provide --{kind}-fixture PATH to a local regular file before creating production resources')
            pending.append((kind,source,dest))
        for kind,source,dest in pending:
            # Reject symlinks and special files even if the source changes after validation.
            fd=os.open(source,os.O_RDONLY|os.O_NONBLOCK|os.O_NOFOLLOW)
            with os.fdopen(fd,'rb') as src:
                if not stat.S_ISREG(os.fstat(src.fileno()).st_mode):
                    raise ValueError(f'--{kind}-fixture must be a local regular file')
                with dest.open('xb') as out:
                    dest.chmod(0o600)
                    shutil.copyfileobj(src,out)
    def sources(self):
        raw=json.loads((self.path/'production_history.json').read_text())['payload']
        return raw.get('tasks',[])
    def bootstrap(self):
        cost=self.run('bootstrap_image_cost',['task','cost','--type','text2image','--model-version','3.2_image_2_5_fast','--resolution','1080p','--schedule-mode','normal'])
        if cost.get('ok'):
            self.submit('bootstrap_image',['task','submit','--type','text2image','--model-version','3.2_image_2_5_fast','--resolution','1080p','--aspect-ratio','1:1','--schedule-mode','normal','--prompt',self.state['prefix']+' A friendly red cartoon robot, full body, front view, white background.'])
    def regressions(self):
        sources=self.sources()
        target_models={'text2image':'3.2_pro_m','text2video':'viduq3.1-drama-std','img2video':'3.2','headtailimg2video':'3.0'}
        headtail=json.loads((self.path/'production_headtail_history.json').read_text())['payload'].get('tasks',[])
        for typ,model in target_models.items():
            if 'rerun_'+typ in self.state['operations']:continue
            if self.active()>=3:break
            candidates=headtail if typ=='headtailimg2video' else sources
            source=next((t for t in candidates if t.get('type')==typ and t.get('settings',{}).get('model_version')==model),None)
            if not source:
                self.state['cases']['F10_'+typ]={'status':'BLOCKED','reason':'no_matching_production_success_source','target_model':model};self.save();continue
            settings=source['settings'];duration='4' if typ=='text2video' else '5' if model=='3.0' else '2'
            cmd=['task','cost','--type',typ,'--model-version',model,'--resolution',settings['resolution'],'--schedule-mode','normal']
            opts=['--prompt',self.state['prefix']+' A friendly red cartoon robot gently waves.']
            if typ!='text2image':cmd+=['--duration',duration];opts+=['--duration',duration]
            if settings.get('transition') not in [None,'unspecified']:cmd+=['--transition',settings['transition']]
            quote=self.run('cost_'+typ,cmd)
            if not quote.get('ok'):continue
            self.state['resources'].setdefault('source_tasks',{})[typ]=source['id'];self.save()
            self.run('source_'+typ,['task','get',source['id'],'--details'])
            self.submit('rerun_'+typ,['task','rerun',source['id'],*opts])
    def parity(self):
        # Exactly two separately named comparisons; never reuse deleted test tasks as sources.
        projections={
            'parity_drama_1080p':{'model_version':'viduq3.1-drama-std','resolution':'1080p','transition':'unspecified','duration':4,'sample_count':1},
            'parity_img2video_speed':{'model_version':'3.2','resolution':'1080p','transition':'speed','duration':2,'sample_count':1}}
        self.state['resources']['parity_expected']=projections
        for name,target in projections.items():
            self.state['cases'].setdefault(name,{'status':'PENDING','expected':target})
            self.state.setdefault('final_cases',{}).setdefault(name,{'status':'PENDING','expected':target})
        self.save()
        drama=self.state['resources'].get('source_tasks',{}).get('text2video')
        speed=None;token='';seen=set()
        for page in range(20):
            if token in seen:break
            seen.add(token)
            d=self.run('parity_speed_history_'+str(page),['task','list','--state','success','--type','img2video','--pagesz','100','--page-token',token])
            if not d.get('ok'):break
            speed=next((t for t in d.get('tasks',[]) if t.get('settings',{}).get('model_version')=='3.2' and t.get('settings',{}).get('transition')=='speed' and t.get('settings',{}).get('resolution')=='1080p'),None)
            if speed:break
            token=d.get('next_page_token')
            if not token:break
        candidates={'parity_drama_1080p':drama,'parity_img2video_speed':speed['id'] if speed else None}
        for name,source_id in candidates.items():
            if name in self.state['operations']:continue
            target=projections[name];typ='text2video' if name=='parity_drama_1080p' else 'img2video'
            if not source_id:
                self.state['cases'][name]={'status':'BLOCKED','reason':'no_matching_owned_production_source','expected':target};self.save();self.emit(name=name,status='BLOCKED',expected=target);continue
            source=self.run('source_'+name,['task','get',source_id,'--details'])
            settings=source.get('settings',{})
            if source.get('state')!='success' or source.get('type')!=typ or settings.get('model_version')!=target['model_version'] or settings.get('sample_count')!=1 or settings.get('transition')!=target['transition'] or (typ=='img2video' and (settings.get('transition')!='speed' or settings.get('resolution')!='1080p')):
                self.state['cases'][name]={'status':'BLOCKED','reason':'source_detail_did_not_match','expected':target};self.save();continue
            quote=['task','cost','--type',typ,'--model-version',target['model_version'],'--duration',str(target['duration']),'--resolution','1080p','--sample-count','1','--schedule-mode','normal']
            if typ=='img2video':quote+=['--transition','speed']
            q=self.run(name+'_cost',quote)
            if q.get('ok'):
                self.state['resources'].setdefault('parity_sources',{})[name]=source_id;self.save()
                self.submit(name,['task','rerun',source_id,'--duration',str(target['duration']),'--resolution','1080p','--prompt',self.state['prefix']+' '+name+' A friendly red cartoon robot gently waves.'])
                self.emit(name=name,expected=target)
    def derived(self):
        image=next((tid for tid,m in self.state['tasks'].items() if m['operation']=='bootstrap_image' and m['state']=='success'),None)
        if image and self.active()<3:
            d=self.run('bootstrap_image_details',['task','get',image,'--details']);cid=d['creations'][0]['id']
            self.state['resources']['image_creation']=cid;self.save()
            q=self.get('upscale_cost','/vidu/v1/tasks/credits',{'type':'upscale_image','input.creation_id':cid,'settings.resolution':'2k'})
            if q.get('can_submit'):self.submit('upscale',['task','upscale-image','--creation-id',cid,'--resolution','2k'])
        source=next((t for t in self.sources() if t.get('settings',{}).get('resolution')=='1080p' and t.get('creations') and t['creations'][0].get('type')=='video'),None)
        if source and self.active()<3:
            cid=source['creations'][0]['id'];self.state['resources']['extend_source_creation']=cid;self.save()
            q=self.get('extend_cost','/vidu/v1/tasks/credits',{'type':'video2extend','settings.duration':'1','settings.model_version':'3.1','settings.resolution':'1080p','settings.sample_count':'1','settings.transition':'extendspeed','settings.schedule_mode':'normal'})
            if q.get('can_submit'):self.submit('extend',['task','extend','--creation-id',cid,'--duration','1','--transition','extendspeed'])
    def api_mutation(self,name,method,path,body=None):
        self.touched.add(name)
        if name in self.state['operations']:
            f=self.path/(name+'.json');return json.loads(f.read_text()).get('payload',{}) if f.exists() else {}
        if not (path in ['/v1/material/labels','/vpp/v1/material/labels','/vidu/v1/material/labels'] and method=='POST' or (path.startswith('/v1/material/labels/') or path.startswith('/vpp/v1/material/labels/') or path.startswith('/vidu/v1/material/labels/')) and method=='DELETE'):
            raise ValueError('raw mutation outside narrow label allowlist')
        self.state['operations'][name]={'command':['label_api',method,path],'write':True,'status':'attempting','started':time.time()};self.save()
        req=urllib.request.Request(BASE+path,data=json.dumps(body).encode() if body is not None else None,method=method,headers={'Authorization':'Token '+self.config['VIDU_TOKEN'],'Content-Type':'application/json'})
        try:
            with urllib.request.urlopen(req,timeout=40) as reply:
                content=reply.read();d=json.loads(content) if content else {}
            ok=True
        except urllib.error.HTTPError as error:
            d={'http_status':error.code,'body':error.read().decode()};ok=False
        except Exception as error:d={'error_type':type(error).__name__};ok=False
        raw=redact_strings({'payload':d},self.config['VIDU_TOKEN']);(self.path/(name+'.json')).write_text(json.dumps(raw,ensure_ascii=False,indent=2))
        self.state['operations'][name]['status']='PASS' if ok else 'FAIL';self.save();self.emit(name=name,status=self.state['operations'][name]['status'])
        return d
    def element(self):
        if 'element_create' not in self.state['operations']:
            self.prepare_fixtures('image')
        d=self.run('element_create',['element','create','--name','cli-regression-prod-'+self.path.name[-8:],'--image',str(self.path/'robot.png')],True,timeout=180)
        eid=d.get('id') or d.get('element',{}).get('id')
        if not eid:return
        eid=str(eid);self.state['resources']['element']={'id':eid,'cleanup':'pending'};self.save()
        original=self.run('element_original',['element','get',eid])
        self.run('element_name',['element','update',eid,'--name','cli-regression-prod-'+self.path.name[-8:]+'-u'],True)
        self.run('element_recaption',['element','update',eid,'--recaption','A friendly red cartoon robot on a white background.'],True)
        catalog=self.get('label_catalog','/v1/material/labels',{'page.pagesz':'20'})
        route='/v1/material/labels'
        if catalog.get('http_status')==404:
            route='/vpp/v1/material/labels';catalog=self.get('label_catalog_vpp',route,{'page.pagesz':'20'})
        self.state['resources']['label_route']=route;self.save()
        labels=catalog.get('labels',[])
        label=str(labels[0]['id']) if labels else None
        if not label and 'http_status' not in catalog:
            created=self.api_mutation('label_create','POST',route,{'name':'cli-regression-prod-'+self.path.name[-8:]})
            label=str(created['id']) if created.get('id') else None
            if label:self.state['resources']['label']={'id':label,'cleanup':'pending'};self.save()
        if label:
            self.state['resources']['legal_label_used']=label;self.save()
            self.run('element_labels_replace',['element','update',eid,'--label',label],True)
            labeled=self.run('element_labeled_get',['element','get',eid])
            self.state['resources']['label_replace_verified']=label in [str(x['id']) for x in labeled.get('element',{}).get('labels',[])]
            self.save()
        self.run('element_labels_clear',['element','update',eid,'--clear-labels'],True)
        updated=self.run('element_updated',['element','get',eid])
        before=original.get('element',{});after=updated.get('element',{})
        style_ok=before.get('recaption',{}).get('style')==after.get('recaption',{}).get('style')
        self.state['cases']['F04_F05_element']={'status':'PASS' if updated.get('ok') and style_ok and not after.get('labels') and after.get('name','').endswith('-u') and after.get('recaption',{}).get('description')=='A friendly red cartoon robot on a white background.' and self.state['resources'].get('label_replace_verified') is True else 'FAIL','style_preserved':style_ok,'label_used':bool(label)};self.save()
    def extend_fixed(self):
        cid=self.state['resources'].get('extend_source_creation')
        if not cid:return
        quote=self.get('extend_after_range_fix_cost','/vidu/v1/tasks/credits',{'type':'video2extend','settings.duration':'1','settings.model_version':'3.1','settings.resolution':'1080p','settings.sample_count':'1','settings.transition':'extendspeed','settings.schedule_mode':'normal'})
        if quote.get('can_submit'):
            self.submit('extend_after_range_fix',['task','extend','--creation-id',cid,'--duration','1','--transition','extendspeed'])
    def element_fixed(self):
        element=self.state['resources'].get('element')
        if not element:return
        eid=element['id']
        # Explicit second attempts only after a reviewed CLI fix; distinct evidence retained.
        self.run('element_name_fixed',['element','update',eid,'--name','cli-regression-prod-'+self.path.name[-8:]+'-u'],True)
        catalog=self.get('label_catalog_vidu','/vidu/v1/material/labels',{'page.pagesz':'20'})
        labels=catalog.get('labels',[])
        label=str(labels[0]['id']) if labels else None
        if label:
            self.state['resources']['legal_label_used']=label;self.save()
            self.run('element_labels_replace_fixed',['element','update',eid,'--label',label],True)
            labeled=self.run('element_labeled_fixed',['element','get',eid])
            self.state['resources']['label_replace_verified']=label in [str(x['id']) for x in labeled.get('element',{}).get('labels',[])];self.save()
        self.run('element_labels_clear_fixed',['element','update',eid,'--clear-labels'],True)
        updated=self.run('element_updated_fixed',['element','get',eid])
        original=json.loads((self.path/'element_original.json').read_text())['payload'].get('element',{})
        after=updated.get('element',{});style_ok=original.get('recaption',{}).get('style')==after.get('recaption',{}).get('style')
        self.state['cases']['F04_F05_element']={'status':'PASS' if updated.get('ok') and style_ok and not after.get('labels') and after.get('name','').endswith('-u') and after.get('recaption',{}).get('description')=='A friendly red cartoon robot on a white background.' and self.state['resources'].get('label_replace_verified') is True else 'FAIL','style_preserved':style_ok,'label_replace_verified':self.state['resources'].get('label_replace_verified'),'clear_verified':not after.get('labels'),'cli_fix_retested':True};self.save()
    def voice(self):
        if 'voice_create' not in self.state['operations']:
            self.prepare_fixtures('audio')
        self.get('voice_cost_corrected','/credit/v1/credits/cost',{'biz_code':'voice'})
        self.get('voice_trials','/credit/v1/trials/me/all',{})
        d=self.run('voice_create',['voice','create','--audio',str(self.path/'generic-synthetic-voice.wav'),'--name','cli-regression-prod-'+self.path.name[-8:],'--wait','--timeout','30'],True,timeout=180)
        vid=d.get('voice_id') or d.get('error',{}).get('fields',{}).get('voice_id')
        if vid:self.state['resources']['voice']={'id':vid,'cleanup':'retained_no_delete_API'};self.save()
    def corrected_probes(self):
        self.get('voice_cost_corrected','/credit/v1/credits/cost',{'biz_code':'voice'})
        if 'voice_cost' in self.state['operations']:
            self.state['operations']['voice_cost']['status']='INVALID_TEST';self.state['operations']['voice_cost']['reason']='wrong biz_code material/voice; correct value voice';self.save()
        self.get('label_catalog_vidu','/vidu/v1/material/labels',{'page.pagesz':'20'})
    def voice_dependents(self):
        voice=self.state['resources'].get('voice')
        if not voice:return
        vid=voice['id']
        self.run('voice_update',['voice','update',vid,'--name','cli-regression-prod-'+self.path.name[-8:]+'-u','--description','Generic synthetic software validation voice; no real person imitation.'],True)
        self.run('voice_wait',['voice','wait',vid,'--timeout','30'])
        self.run('voice_get',['voice','get',vid])
        self.run('voice_list',['voice','list','--pagesz','100'])
        quote=self.run('personal_tts_cost',['task','tts-cost','--voice-id',vid,'--text','你好，这是软件回归测试的通用朗读。','--schedule-mode','normal'])
        if quote.get('ok'):self.submit('personal_tts',['task','tts','--voice-id',vid,'--prompt','你好，这是软件回归测试的通用朗读。','--schedule-mode','normal'])
        video=self.path/'lipsync-source.mp4'
        if video.exists() and self.active()<3:
            q=self.run('personal_lipsync_cost',['task','lip-sync-cost','--duration',str(__import__('math').ceil(float(self.state.get('media_validation',{}).get('source_video',{}).get('duration',5)))),'--voice-id',vid,'--schedule-mode','normal'])
            if q.get('ok'):self.submit('personal_lipsync',['task','lip-sync','--video',str(video),'--text','你好，这是软件回归测试的通用朗读。','--voice-id',vid,'--schedule-mode','normal'])
    def download_source(self):
        # Existing production assets are only read; select a short owned successful video.
        sources=self.sources()
        source=next((t for t in sources if t.get('settings',{}).get('duration',999)<=8 and t.get('creations') and t['creations'][0].get('type')=='video'),None)
        if not source:return
        self.state['resources']['lipsync_readonly_source_task']=source['id'];self.save()
        self.run('production_video_download',['task','get',source['id'],'--details','--output',str(self.path/'lipsync-source.mp4')],timeout=180)
        if (self.path/'lipsync-source.mp4').exists():self.verify_media('source_video',self.path/'lipsync-source.mp4')
    def verify_media(self,name,path):
        probe=shutil.which('ffprobe')
        if not probe:raise RuntimeError('ffprobe required for production media validation')
        p=subprocess.run([probe,'-v','error','-show_streams','-show_format','-of','json',str(path)],capture_output=True,text=True,timeout=40)
        data=json.loads(p.stdout) if p.stdout.strip() else {}
        (self.path/(name+'_ffprobe.json')).write_text(json.dumps({'exit_code':p.returncode,'data':data,'stderr':p.stderr}))
        valid=p.returncode==0 and path.stat().st_size>0 and bool(data.get('streams'))
        self.state.setdefault('media_validation',{})[name]={'ok':valid,'bytes':path.stat().st_size,'duration':data.get('format',{}).get('duration')};self.save();return valid
    def recording_upload_id(self):
        import re
        f=self.path/'voice_create.json'
        if not f.exists():return None
        log=json.loads(f.read_text()).get('stderr','')
        match=re.search(r'录音上传 ID: (\d+)',log)
        return match.group(1) if match else None
    def charges(self):
        super().charges()
        voice=self.state['resources'].get('voice',{}).get('id')
        if voice:
            latest=sorted(self.path.glob('ledger_*_0.json'))[-1]
            prefix=latest.name[:-len('0.json')]
            records=[]
            for page in sorted(self.path.glob(prefix+'*.json')):
                records+=json.loads(page.read_text()).get('payload',{}).get('records',[])
            matches=[];seen=set();own_tasks=set(self.state['tasks'])
            for record in records:
                record_id=str(record.get('id',''))
                if record_id in seen or str(record.get('task_id')) in own_tasks:continue
                if record.get('remark')=='voice:'+voice:
                    seen.add(record_id)
                    matches.append({'record_id':record_id,'amount':record.get('amount',0),'matched_fields':['remark']})
            amount=-sum(r['amount'] for r in matches)
            self.state['charges']['voice']={'voice_id':voice,'matched_records':matches,'net_spent':amount if matches else None,'attribution':'remark exactly equals voice:<full_voice_id>; own task rows excluded; all current ledger pages deduplicated by record_id'}
            self.state['charges']['total_attributed']=self.state['charges']['net_spent']+amount if matches else None
            self.save();self.emit(voice_ledger_matches=len(matches),voice_net_spent=amount if matches else None,total_attributed=self.state['charges']['total_attributed'])
    def finalize(self):
        self.poll()
        for tid,meta in self.state['tasks'].items():
            op=meta['operation']
            if meta.get('cleanup')=='deleted':
                files=sorted(self.path.glob('final_details_'+tid+'_*'));d=json.loads(files[-1].read_text())['payload'] if files else {}
            else:d=self.run('final_details_'+tid+'_'+str(int(time.time())),['task','get',tid,'--details'])
            if d.get('state'):meta['state']=d['state']
            status='PASS' if meta['state']=='success' else 'FAIL' if meta['state'] in ['failed','canceled'] else 'PENDING'
            if op.startswith('parity_'):
                source=json.loads((self.path/('source_'+op+'.json')).read_text())['payload'];equivalent=self.media_equivalent(source,d)
                expected=self.state['resources']['parity_expected'][op]
                actual={k:d.get('settings',{}).get(k) for k in expected}
                exact=actual==expected
                combined=status if status!='PASS' else 'FAIL' if equivalent is False or not exact else 'BLOCKED' if equivalent is None else 'PASS'
                self.state['cases'][op]={'status':combined,'state':meta['state'],'generation_status':status,'expected':expected,'actual':actual,'exact_params':exact,'reference_identity_preserved':equivalent,'task_err_code':d.get('err_code'),'task_err_msg':d.get('err_msg')}
            elif op.startswith('rerun_'):
                typ=op[6:];source=json.loads((self.path/('source_'+typ+'.json')).read_text())['payload'];equivalent=self.media_equivalent(source,d)
                model_ok=source.get('settings',{}).get('model_version')==d.get('settings',{}).get('model_version')
                self.state['cases']['F10_'+typ]={'status':status if status!='PASS' or equivalent is True and model_ok else 'FAIL' if equivalent is False or not model_ok else 'BLOCKED','state':meta['state'],'generation_status':status,'reference_identity_preserved':equivalent,'model_preserved':model_ok,'source_environment':BASE,'task_err_code':d.get('err_code')}
            else:self.state['cases'][op]={'status':status,'state':meta['state'],'task_err_code':d.get('err_code')}
            if meta['state']=='success':
                self.run('download_'+tid,['task','get',tid,'--details','--output',str(self.path/('output-'+tid))],timeout=180)
                for media in (self.path/('output-'+tid)).glob('*'):
                    if media.suffix.lower() in ['.png','.jpeg','.jpg','.webp','.mp4','.mov','.mp3','.wav','.m4a']:
                        self.verify_media(tid+'_'+media.suffix[1:],media)
            if meta['state']=='success':
                media_rows=[v for k,v in self.state.get('media_validation',{}).items() if k.startswith(tid+'_')]
                media_ok=bool(media_rows) and all(v.get('ok') for v in media_rows)
                key='F10_'+op[6:] if op.startswith('rerun_') else op
                self.state['cases'][key]['media_validated']=media_ok
                if not media_ok:self.state['cases'][key]['status']='FAIL'
            self.save()
        voice=self.state['resources'].get('voice')
        if voice:
            listed=json.loads((self.path/'voice_list.json').read_text())['payload'] if (self.path/'voice_list.json').exists() else {}
            item=next((v for v in listed.get('voices',[]) if v.get('voice_id')==voice['id']),{})
            metadata_ok=item.get('name')=='cli-regression-prod-'+self.path.name[-8:]+'-u' and item.get('description')=='Generic synthetic software validation voice; no real person imitation.'
            self.state['cases']['F12_voice_metadata']={'status':'PASS' if metadata_ok and self.state['operations'].get('voice_wait',{}).get('status')=='PASS' and self.state['operations'].get('voice_get',{}).get('status')=='PASS' else 'FAIL','name_description_verified':metadata_ok}
        recording_count=int(self.recording_upload_id() is not None)
        image_count=int(bool(self.state['resources'].get('element')))
        lipsync_count=int(any(m['operation']=='personal_lipsync' for m in self.state['tasks'].values()))
        self.state['resources']['upload_retention']={'count':recording_count+image_count+lipsync_count,'recording':{'count':recording_count,'upload_id':self.recording_upload_id(),'retained_with_voice':bool(voice)},'element_image':{'count':image_count,'upload_id':None,'id_visibility':'not_exposed_by_CLI'},'lipsync_video':{'count':lipsync_count,'upload_id':None,'id_visibility':'not_exposed_by_CLI'},'cleanup_interface_in_CLI':False}
        self.state['cases']['F12_clone']={'status':'PASS' if voice and self.state['operations'].get('voice_create',{}).get('status')=='PASS' else 'FAIL','new_voice_retained':bool(voice)}
        self.state['cases']['voice_cost_corrected']={'status':'PASS' if self.state['operations'].get('voice_cost_corrected',{}).get('status')=='PASS' else 'FAIL','initial_probe':'INVALID_TEST_wrong_biz_code'}
        self.state['cases']['production_video_download']={'status':'PASS' if self.state.get('media_validation',{}).get('source_video',{}).get('ok') else 'FAIL'}
        self.save();self.emit(cases=self.state['cases'],media_validation=self.state.get('media_validation'))
    def summary(self):
        self.finalize()
        final={}
        mapping={'F09_upscale':'upscale','F10_text2image':'F10_text2image','F10_text2video':'F10_text2video','F10_img2video':'F10_img2video','F10_headtailimg2video':'F10_headtailimg2video','F04_F05_element':'F04_F05_element','F12_clone':'F12_clone','F12_voice_metadata':'F12_voice_metadata','F12_personal_tts':'personal_tts','F12_personal_lipsync':'personal_lipsync','voice_cost':'voice_cost_corrected','source_video_download':'production_video_download','F08_extend':'extend_after_range_fix'}
        for name,source in mapping.items():final[name]=dict(self.state['cases'].get(source,{'status':'PENDING'}))
        for name in ['parity_drama_1080p','parity_img2video_speed']:
            if name in self.state['resources'].get('parity_expected',{}):final[name]=dict(self.state['cases'].get(name,{'status':'PENDING'}))
        repaired=next((tid for tid,m in self.state['tasks'].items() if m['operation']=='extend_after_range_fix'),None)
        if repaired:
            history=json.loads((self.path/'production_history.json').read_text())['payload']
            source=next((c for t in history.get('tasks',[]) for c in t.get('creations',[]) if str(c.get('id'))==self.state['resources'].get('extend_source_creation')),None)
            source_duration=float(source.get('video',{}).get('duration') or source.get('duration') or 0) if source else 0
            output=self.state.get('media_validation',{}).get(repaired+'_mp4',{})
            result_duration=float(output.get('duration') or 0)
            growth_ok=source_duration>0 and result_duration>source_duration and abs(result_duration-(source_duration+1))<=0.5
            final['F08_extend'].update({'source_duration':source_duration,'result_duration':result_duration,'duration_growth_valid':growth_ok})
            if final['F08_extend'].get('status')=='PASS' and not growth_ok:final['F08_extend']['status']='FAIL'
        final['F05_label_replace_clear']={'status':'PASS' if self.state['resources'].get('label_replace_verified') and self.state['cases'].get('F04_F05_element',{}).get('clear_verified') else 'FAIL','used_existing_catalog_label':bool(self.state['resources'].get('legal_label_used')) and not bool(self.state['resources'].get('label'))}
        complete=all(t.get('cleanup')=='deleted' and t.get('history_absent') is True for t in self.state['tasks'].values())
        element=self.state['resources'].get('element');label=self.state['resources'].get('label')
        complete=complete and (not element or element.get('cleanup')=='deleted' and element.get('list_absent') is True) and (not label or label.get('cleanup')=='deleted')
        final['cleanup']={'status':'PASS' if complete else 'PENDING','task_count':len(self.state['tasks']),'tasks_deleted':sum(t.get('cleanup')=='deleted' for t in self.state['tasks'].values()),'active_tasks':self.active(),'element_deleted':element.get('cleanup')=='deleted' if element else None}
        self.state['final_cases']=final
        self.state['before_fix_evidence']={'element_name':'element_name.json','element_clear':'element_labels_clear.json','extend':'extend.json','extend_terminal':str(sorted(self.path.glob('poll_'+next((tid for tid,m in self.state['tasks'].items() if m['operation']=='extend'),'missing')+'_*'))[-1].name) if any(m['operation']=='extend' for m in self.state['tasks'].values()) else None,'wrong_cost_probe':'voice_cost.json INVALID_TEST: wrong biz_code material/voice'}
        self.save();self.emit(final_cases=final,charges=self.state.get('charges'),residuals=self.state['resources'].get('upload_retention'))
    def cleanup(self):
        super().cleanup()
        element=self.state['resources'].get('element')
        if element and element.get('cleanup')=='deleted':
            listed=self.run('elements_after_cleanup_prod_'+str(int(time.time())),['element','list','--pagesz','100'])
            element['list_absent']=listed.get('ok') is True and str(element['id']) not in [str(e['id']) for e in listed.get('elements',[])];self.save()
        label=self.state['resources'].get('label')
        if label and label.get('cleanup')!='deleted':
            created=json.loads((self.path/'label_create.json').read_text()).get('payload',{})
            if str(created.get('id'))!=str(label['id']):raise ValueError('label cleanup ownership mismatch')
            d=self.api_mutation('label_delete','DELETE',self.state['resources'].get('label_route','/v1/material/labels')+'/'+label['id'])
            if (self.path/'label_delete.json').exists() and self.state['operations'].get('label_delete',{}).get('status')=='PASS' and 'http_status' not in d and 'error_type' not in d:label['cleanup']='deleted';self.save()
    def inspect(self):
        self.poll()
        self.emit(state_dir=str(self.path),tasks=self.state['tasks'],cases=self.state['cases'])
    def dispatch(self):
        getattr(self,self.args.phase)()
        self.emit(state_dir=str(self.path),tasks=self.state['tasks'])
        bad=any(self.state['operations'][name].get('status') in ['FAIL','attempting'] for name in self.touched)
        if self.args.phase in ['inspect','cleanup','summary'] and self.active():bad=True
        if self.args.phase=='summary' and any(c.get('status')!='PASS' for c in self.state.get('final_cases',{}).values()):bad=True
        if self.args.phase!='summary' and any(c.get('status') in ['FAIL','BLOCKED','PENDING'] for c in self.state['cases'].values()):bad=True
        if bad:raise SystemExit(1)

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--execute',action='store_true')
    p.add_argument('--state-dir',type=Path)
    p.add_argument('--env-file',type=Path,default=ROOT/'.env.cn.prod')
    p.add_argument('--preflight-dir',type=Path)
    p.add_argument('--image-fixture',type=Path,help='local image file to copy into private production state (reused on resume)')
    p.add_argument('--audio-fixture',type=Path,help='local generic synthetic voice audio to copy into private production state (reused on resume)')
    p.add_argument('--phase',choices=['setup','bootstrap','regressions','derived','inspect','charges','cleanup','element','voice','voice_dependents','download_source','corrected_probes','element_fixed','finalize','extend_fixed','summary','parity'],default='setup')
    args=p.parse_args()
    if not args.execute:
        print('Dry-run only: production network and writes require --execute.');return
    ProductionRunner(args).dispatch()
if __name__=='__main__':main()
