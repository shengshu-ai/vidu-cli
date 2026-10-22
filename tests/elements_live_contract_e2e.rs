//! Pagination contracts confirmed against the live API, reproduced without credentials.
use assert_cmd::Command;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;
use std::time::{Duration, Instant};

fn command(base: &str) -> Command {
    let mut cmd = Command::cargo_bin("vidu-cli").unwrap();
    cmd.env("VIDU_BASE_URL", base)
        .env("VIDU_TOKEN", "pagination-contract-test")
        .env_remove("VIDU_DEBUG")
        .timeout(Duration::from_secs(10));
    cmd
}

#[test]
fn shared_search_and_favorites_send_both_page_size_boundaries() {
    for pagesz in ["1", "100"] {
        for search in [true, false] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            let server = thread::spawn(move || {
                let deadline = Instant::now() + Duration::from_secs(5);
                let mut socket = loop {
                    match listener.accept() {
                        Ok((socket, _)) => break socket,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < deadline, "expected feed HTTP request");
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(e) => panic!("accept failed: {e}"),
                    }
                };
                socket.set_nonblocking(false).unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                socket
                    .set_write_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut request = Vec::new();
                let mut buffer = [0; 2048];
                while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                    let n = socket.read(&mut buffer).unwrap();
                    assert!(n > 0, "connection closed before request headers");
                    request.extend_from_slice(&buffer[..n]);
                    assert!(request.len() < 65536);
                }
                let head = String::from_utf8(request).unwrap();
                let mut line = head.lines().next().unwrap().split_whitespace();
                assert_eq!(line.next(), Some("GET"));
                let url = reqwest::Url::parse(&format!("http://localhost{}", line.next().unwrap()))
                    .unwrap();
                assert_eq!(url.path(), "/vidu/v1/material/share_elements/feed");
                let actual: BTreeMap<String, String> = url
                    .query_pairs()
                    .map(|(k, v)| (k.into_owned(), v.into_owned()))
                    .collect();
                let mut expected = BTreeMap::from([
                    ("pager.pagesz".to_string(), pagesz.to_string()),
                    ("pager.page_token".to_string(), "cursor".to_string()),
                    ("is_collect".to_string(), (!search).to_string()),
                ]);
                if search {
                    for (key, value) in [
                        ("keyword", "cat"),
                        ("sort_by", "recommend"),
                        ("modalities", "image"),
                        ("is_like", "false"),
                    ] {
                        expected.insert(key.into(), value.into());
                    }
                }
                assert_eq!(actual, expected);
                let body = json!({"share_elements":[{"element":{"id":"123","version":"456","name":"fixture"}}],"next_page_token":"next"}).to_string();
                write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
            });
            let mut cmd = command(&base);
            cmd.args([
                "element",
                if search { "search" } else { "favorites" },
                "--pagesz",
                pagesz,
                "--page-token",
                "cursor",
            ]);
            if search {
                cmd.args(["--keyword", "cat"]);
            }
            let output = cmd.output().unwrap();
            server.join().expect("feed request contract failed");
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let result: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(result["ok"], true);
            assert_eq!(result["elements"][0]["id"], "123");
            assert_eq!(result["next_page_token"], "next");
        }
    }
}

#[test]
fn invalid_page_sizes_fail_before_any_http_request() {
    for (action, sizes) in [
        ("search", &["0", "101"][..]),
        ("favorites", &["0", "101"][..]),
        ("list", &["1", "9", "101"][..]),
    ] {
        for pagesz in sizes {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let mut cmd = command(&format!("http://{}", listener.local_addr().unwrap()));
            cmd.args(["element", action, "--pagesz", pagesz]);
            if action == "search" {
                cmd.args(["--keyword", "cat"]);
            }
            let output = cmd.output().unwrap();
            assert!(!output.status.success());
            assert!(String::from_utf8_lossy(&output.stderr).contains("--pagesz"));
            assert!(
                matches!(listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock),
                "invalid page size sent HTTP request"
            );
        }
    }
}
