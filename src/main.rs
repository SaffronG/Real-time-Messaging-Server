use std::{
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    env,
};

use chrono::Local;
use serde_json::json;
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::oneshot;
use serde_json;

#[derive(serde::Deserialize)]
struct Message {
    logs: String,
}

struct HttpResponse {
    status_code: u16,
    status_text: String,
    headers: Vec<(String, String)>,
    body: String,
}

impl HttpResponse {
    fn new(status_code: u16, status_text: &str, headers: Vec<(String, String)>, body: &str) -> Self {
        let mut full_headers = headers;
        full_headers.push(("Content-Length".into(), body.len().to_string()));

        HttpResponse {
            status_code,
            status_text: status_text.into(),
            headers: full_headers,
            body: body.into(),
        }
    }

    fn as_bytes(&self) -> Vec<u8> {
        let mut response = format!("HTTP/1.1 {} {}\r\n", self.status_code, self.status_text);
        for (k, v) in &self.headers {
            response.push_str(&format!("{}: {}\r\n", k, v));
        }
        response.push_str("\r\n");
        response.push_str(&self.body);
        response.into_bytes()
    }
}

fn read_request_line(stream: &mut TcpStream) -> Option<String> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    if reader.read_line(&mut line).is_ok() {
        let parts: Vec<&str> = line.trim().split_whitespace().collect();
        if parts.len() >= 2 {
            return Some(parts[1].to_string());
        }
    }
    None
}

fn handle_client(fname: String, mut stream: TcpStream) {
    let uri = match read_request_line(&mut stream) {
        Some(uri) => uri,
        None => return,
    };

    let response = if uri.contains('?') {
        let parts: Vec<&str> = uri.splitn(2, '?').collect();
        let message = parts[1];

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&fname)
            .unwrap();
        let time = Local::now();
        writeln!(file, "{} {}", time.format("%Y/%m/%d %H:%M:%S").to_string(), message).unwrap();

        let mut content = String::new();
        File::open(fname).unwrap().read_to_string(&mut content).unwrap();

        let body = json!({ "logs": content }).to_string();
        HttpResponse::new(200, "OK", vec![("Content-Type".into(), "application/json".into())], &body)
    } else if uri == "/logs" {
        let body = match std::fs::read_to_string(fname) {
            Ok(content) => json!({ "logs": content }).to_string(),
            Err(_) => json!({ "error": "No logs found" }).to_string(),
        };
        HttpResponse::new(200, "OK", vec![("Content-Type".into(), "application/json".into())], &body)
    } else {
        HttpResponse::new(404, "Not Found", vec![("Content-Type".into(), "application/json".into())], r#"{"error": "Invalid URL"}"#)
    };

    let _ = stream.write_all(&response.as_bytes());
}

fn run_server() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind port");
    let addr = listener.local_addr().unwrap();
    let mut fname = String::new();
    print!("Enter a filename to store the logs (eg. logs.txt): ");
    std::io::stdout().flush().expect("Failed to flush!");
    std::io::stdin().read_line(&mut fname).unwrap();
    let fname = fname.trim();
    match File::open(&fname) {
        Ok(_) => println!("Opened {fname} log file"),
        Err(_) => {
            match File::create(&fname) {
                Ok(_) => println!("Created {fname} log file"),
                Err(_) => println!("Failed to create file!"),
            }
        },
    }
    println!("Server running at: http://{}", addr);

    for stream in listener.incoming() {
        if let Ok(stream) = stream {
            handle_client(fname.trim().to_string().clone(), stream);
        }
    }
}

fn parse_address(addr: &str) -> (String, u16) {
    let mut parts = addr.split(':');
    let ip = parts.next().unwrap().to_string();
    let port = parts.next().unwrap().parse().unwrap();
    (ip, port)
}

async fn send_msg(ip: &str, port: u16, user: &str, msg: &str) {
    let url = format!("http://{}:{}/{}?{}", ip, port, user, msg.trim());
    let _ = reqwest::get(&url).await;
}

async fn get_logs(ip: &str, port: u16) -> std::io::Result<String> {
    let url = format!("http://{}:{}/logs", ip, port);
    match reqwest::get(&url).await {
        Ok(resp) => match resp.text().await {
            Ok(text) => Ok(text),
            Err(_) => Err(std::io::ErrorKind::InvalidData.into()),
        },
        Err(_) => Err(std::io::ErrorKind::ConnectionRefused.into()),
    }
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        run_server();
    } else {
        let user = &args[1];

        println!("Enter server address (e.g. 127.0.0.1:PORT): ");
        std::io::stdout().flush().unwrap();

        let mut input = String::new();
        std::io::stdin().read_line(&mut input).unwrap();
        let (ip, port) = parse_address(input.trim());

        let (shutdown_tx, mut shutdown_rx) = oneshot::channel();

        let mut sigint = signal(SignalKind::interrupt()).unwrap();

        let user_ref = user.clone();
        let ip_ref = ip.clone();
        let ip_ref_b = ip.clone();
        tokio::spawn(async move {
            sigint.recv().await;
            println!("Shutting down...");
            shutdown_tx.send(()).unwrap();
        });

        let reader = tokio::spawn(async move {
            let mut join_msg = false; 
            loop {
                let mut msg = String::new();
                if join_msg {
                    if std::io::stdin().read_line(&mut msg).is_ok() {
                        send_msg(&ip_ref, port, &user_ref, &format!("{}: {}", &user_ref,&msg)).await;
                    }
                } else {
                    send_msg(&ip_ref, port, &user_ref, &format!("{}: {}", &user_ref, &format!("{} joined...", &user_ref))).await;
                    join_msg = true;
                }
            }
        });

        let printer = tokio::spawn(async move {
            let mut msg_cache: Vec<String> = Vec::new();

            loop {
                let response = get_logs(&ip_ref_b, port).await.expect("Invalid string response");
                let msg: Message = serde_json::from_str(&response).expect("Failed to parse JSON!");

                let logs: Vec<String> = msg.logs.split("\n").map(|m| urlencoding::decode(m).expect("Failed to decode!").to_string()).collect();

                if logs.len() != msg_cache.len() && !logs.is_empty() {
                    clearscreen::clear().expect("Failed to clear screen!");
                    msg_cache = logs.clone();
                    for line in logs.iter() {
                        println!("{} ", line);
                    }
                    println!();
                }
            }
        });

        tokio::select! {
            _ = &mut shutdown_rx => {
                reader.abort();
                printer.abort();
                println!("Exited, press enter to return to the terimal...");
            }
        }
    }
}

