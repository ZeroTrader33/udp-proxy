use std::env;
use std::net::{SocketAddr, UdpSocket, IpAddr, Ipv4Addr};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use serde_json::{json, Value};
use solana_sdk::transaction::VersionedTransaction;


const JITO_ENDPOINT_1: &str = "https://amsterdam.mainnet.block-engine.jito.wtf/api/v1/bundles";
const JITO_ENDPOINT_2: &str = "https://frankfurt.mainnet.block-engine.jito.wtf/api/v1/bundles";
const JITO_ENDPOINT_NY: &str = "https://ny.mainnet.block-engine.jito.wtf/api/v1/bundles";
const JITO_ENDPOINT_TOKYO: &str = "https://tokyo.mainnet.block-engine.jito.wtf";
const JITO_ENDPOINT_SLC: &str = "https://slc.mainnet.block-engine.jito.wtf";
const UDP_SERVER_IP:&str = "95.217.109.156";
pub struct UdpProxyClient{
    socket: UdpSocket,
    req_client: reqwest::Client,
    tokio_runtime: tokio::runtime::Runtime,
    jito_endpoint_is_1: AtomicBool
}
impl UdpProxyClient {
    pub fn create()->Self {
        let bind_addr: SocketAddr = "0.0.0.0:9999".parse().expect("must be a valid ip:port string");
        let socket = UdpSocket::bind(bind_addr).unwrap();
        let tokio_runtime: tokio::runtime::Runtime = tokio::runtime::Builder::new_current_thread()
            .worker_threads(4)
            // .on_thread_start(move || renice_this_thread(rpc_niceness_adj).unwrap())
            .thread_name("sendBundleThread")
            .enable_all()
            .build()
            .expect("Runtime");
        UdpProxyClient {
            socket,
            req_client: reqwest::Client::new(),
            tokio_runtime,
            jito_endpoint_is_1: AtomicBool::new(true)
        }
    }
    pub fn run() {
        let udp_client = UdpProxyClient::create();
        let arc_udp_client = Arc::new(udp_client);
        loop {
            UdpProxyClient::rec_send_bundle(&arc_udp_client);
        }
    }
    pub fn rec_send_bundle(instance: &Arc<UdpProxyClient>) {
        let mut buf = [0; 10240];

        let receive_res = instance.socket.recv_from(&mut buf);
        if receive_res.is_ok() {
            let (amt, src_addr) = receive_res.unwrap();
            let buf = &mut buf[..amt];
            let result = String::from_utf8_lossy(&buf).to_string();
            
            // println!("received -> {}, {:#?}", result, src_addr.to_string());
            if src_addr.ip().to_string().eq(UDP_SERVER_IP) {
                // println!("received real -> {}", result);
                let jito_endpoint_is_1 = instance.jito_endpoint_is_1.load(Ordering::SeqCst);
                instance.jito_endpoint_is_1.store(!jito_endpoint_is_1, Ordering::SeqCst);
                let endpoint = if jito_endpoint_is_1 {JITO_ENDPOINT_1} else {JITO_ENDPOINT_2};
                UdpProxyClient::send_bundle(instance, result.clone(), endpoint);
                UdpProxyClient::send_bundle(instance, result.clone(), JITO_ENDPOINT_NY);
                UdpProxyClient::send_bundle(instance, result.clone(), JITO_ENDPOINT_TOKYO);
                UdpProxyClient::send_bundle(instance, result.clone(), JITO_ENDPOINT_SLC);
            }
        }
    }
    pub fn send_bundle(instance: &Arc<UdpProxyClient>, payload: String, jito_endpoint: &str) {
        let instance_clone = instance.clone();
        let payload_json = serde_json::from_str::<serde_json::Value>(&payload);
        if payload_json.is_ok() {
            instance.tokio_runtime.block_on(async move {
                let res = instance_clone.req_client
                .post(jito_endpoint)
                .header("Content-Type", "application/json")
                .json(&payload_json.unwrap()) // Serialize the payload to JSON
                .send()
                .await;
                if res.is_ok() {
                    println!("sendBundle success {:#?}", res.unwrap().text().await);
                }
                else {
                    println!("sendBundle Error {:#?}", res.err());
                }
                
            });
        }
        else {
            println!("convert to serde json error");
        }
        
    }
}

const UDP_ENDPOINTS: [&str; 7] = [
    "65.108.21.18:9999",
    "65.108.20.32:9999",
    "65.108.16.32:9999",
    "95.217.81.76:9999",
    "135.181.10.244:9999",
    "135.181.185.146:9999",
    "135.181.135.164:9999"
];
#[derive(Debug)]
pub struct BundleSender {
    socket: UdpSocket,
    client_endpoints: Vec<SocketAddr>,
    req_client: Arc<reqwest::Client>,
    counter: AtomicU32,
    unit: u32,
    tokio_runtime: tokio::runtime::Runtime
}
impl BundleSender {
    pub fn create() -> Self {
        let tokio_runtime: tokio::runtime::Runtime = tokio::runtime::Builder::new_current_thread()
            .worker_threads(4)
            // .on_thread_start(move || renice_this_thread(rpc_niceness_adj).unwrap())
            .thread_name("sendBundleThread")
            .enable_all()
            .build()
            .expect("Runtime");
        let bind_addr: SocketAddr = "0.0.0.0:0".parse().expect("must be a valid ip:port string");
        let socket = UdpSocket::bind(bind_addr).expect("udp socket binding error");

        let mut client_endpoints = Vec::new();
        for endpoint in UDP_ENDPOINTS {
            // let bind_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(endpoint.0, endpoint.1, endpoint.2, endpoint.3)), PORT);
            let bind_addr: SocketAddr = endpoint.parse()
                .expect("must be a valid ip:port string");
            client_endpoints.push(bind_addr);
        }
        BundleSender{
            socket,
            client_endpoints,
            req_client: Arc::new(reqwest::Client::new()),
            counter: AtomicU32::new(0),
            unit: (UDP_ENDPOINTS.len() as u32 + 1) * 2,
            tokio_runtime
        }
    }
    pub fn send_bundle(&self, versioned_txs: &[VersionedTransaction]) {
        let serialized_txs: Vec<Vec<u8>> = versioned_txs
            .iter()
            .map(|tx| bincode::serialize(tx).unwrap())
            .collect();
        let encoded_txs: Vec<String> = serialized_txs
            .iter()
            .map(|tx| bs58::encode(tx).into_string())
            .collect();
        let payload = json!({
            "jsonrpc": "2.0",
            "id": uuid::Uuid::new_v4().to_string(),
            "method": "sendBundle",
            "params": [encoded_txs]
        });
        let payload_str = payload.to_string();
        let counter = self.counter.load(Ordering::SeqCst) % self.unit;
        self.counter.store(counter + 1, Ordering::SeqCst);

        if counter == 0 { // self sending
            self.send_bundle_self(&payload, JITO_ENDPOINT_1);
        }
        else if counter == 1 { // self sending
            self.send_bundle_self(&payload, JITO_ENDPOINT_2);
        }
        else {
            self.send_bundle_udp(payload_str, counter);
        }
    }
    pub fn send_bundle_udp(&self, payload: String, counter: u32) {
        let send_res = self.socket.send_to(payload.as_bytes(), &self.client_endpoints[(counter / 2) as usize - 1]);
        // let send_res = self.socket.send_to(payload.as_bytes(), "65.108.20.32:9999");
        if send_res.is_err() {
            println!("sending udp bundle error {:#?}", send_res.err());
        }
    }
    pub fn send_bundle_self(&self, payload: &Value, jito_endpoint: &str) {
        let client = Arc::clone(&self.req_client);
        self.tokio_runtime.block_on(async move {
            let res = client
            .post(jito_endpoint)
            .header("Content-Type", "application/json")
            .json(payload) // Serialize the payload to JSON
            .send()
            .await;
            if res.is_ok() {
                println!("sendBundle success {:#?}", res.unwrap().text().await);
            }
            else {
                println!("sendBundle Error {:#?}", res.unwrap().text().await);
            }
            
        });
    }

}


// fn main() {
//     std::panic::set_hook(Box::new(|info| {
//         println!("Caught a panic: {:?}", info);
//     }));
//     let mut args = env::args().skip(1);
//     match args.next().as_ref().map(|a| a.as_str()) {
//         Some("server") => {
            
//             UdpProxyClient::run();
//         }
//         Some("client") => {
            
//             run_test_client();
//         }
//         _ => panic!("Invalid usage: [client|server]"),
//     }
// }
// pub fn run_test_client(){
//     let bundle_sender = BundleSender::create();
//     bundle_sender.send_bundle_udp("hello, this is udp bundle sender!".to_string(), 1);
// }
