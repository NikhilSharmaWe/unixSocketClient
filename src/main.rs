use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use thiserror::Error;

const OP_PUT: u8 = 1;
const OP_GET: u8 = 2;
const OP_DELETE: u8 = 3;
const OP_WRITE: u8 = 4;

const STATUS_SUCCESS: u8 = 1;
const STATUS_ERROR: u8 = 0;

const SOCKET_PATH: &str = "/tmp/scalerize";

#[derive(Error, Debug)]
pub enum ClientError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Operation failed: {0}")]
    OperationFailed(String),
    #[error("Invalid response from server: {0}")]
    InvalidResponse(String),
}

pub struct ScalerizeClient {
    stream: UnixStream,
}

impl ScalerizeClient {
    pub fn connect() -> Result<Self, ClientError> {
        let stream = UnixStream::connect(SOCKET_PATH)?;
        Ok(Self { stream })
    }

    fn log_response(response: &[u8]) {
        if response.is_empty() {
            println!("Empty response received");
            return;
        }

        let status = response[0];
        let data = &response[1..];
        
        println!("Server Response Status: {}", status);
        // println!("Raw Response Data: {:?}", data);
        if let Ok(text) = String::from_utf8(data.to_vec()) {
            println!("Response as text: {}", text);
        }
    }

    fn read_full_response(&mut self) -> Result<Vec<u8>, ClientError> {
        let mut response = vec![0u8; 4096];
        let n = self.stream.read(&mut response)?;
        response.truncate(n);
        
        if response.is_empty() {
            return Err(ClientError::InvalidResponse("Empty response from server".to_string()));
        }
        
        Self::log_response(&response);
        Ok(response)
    }

    pub fn get(&mut self, store_number: u8, key: &[u8]) -> Result<Vec<u8>, ClientError> {
        let mut request = vec![OP_GET];
        request.extend_from_slice(&store_number.to_be_bytes());
        
        let key_len = key.len() as u32;
        request.extend_from_slice(&key_len.to_be_bytes());
        request.extend_from_slice(key);
        
        println!("GET REQUEST: {:?}", request);
        self.stream.write_all(&request)?;
        self.stream.flush()?;

        let response = self.read_full_response()?;
        // println!("RESPONSE FOR GET: {:?}", response);
        let status = response[0];
        let data = response[1..].to_vec();

        match status {
            STATUS_SUCCESS => Ok(data),
            STATUS_ERROR => Err(ClientError::OperationFailed(String::from_utf8_lossy(&data).into_owned())),
            _ => Err(ClientError::InvalidResponse(format!("Unexpected status: {}, response: {:?}", status, data)))
        }
    }

    pub fn put(&mut self, store_number: u8, key: &[u8], value: &[u8]) -> Result<(), ClientError> {
        let mut request = vec![OP_PUT];
        request.extend_from_slice(&store_number.to_be_bytes());
        
        let key_len = key.len() as u32;
        request.extend_from_slice(&key_len.to_be_bytes());
        request.extend_from_slice(key);
        
        let value_len = value.len() as u32;
        request.extend_from_slice(&value_len.to_be_bytes());
        request.extend_from_slice(value);
        
        println!("PUT REQUEST: {:?}", request);
        self.stream.write_all(&request)?;
        self.stream.flush()?;

        let response = self.read_full_response()?;
        // println!("RESPONSE FOR PUT: {:?}", response);
        if response[0] == STATUS_ERROR {
            let error_msg = String::from_utf8_lossy(&response[1..]).into_owned();
            return Err(ClientError::OperationFailed(error_msg));
        }

        Ok(())
    }

    pub fn delete(&mut self, store_number: u8, key: &[u8]) -> Result<(), ClientError> {
        let mut request = vec![OP_DELETE];
        request.extend_from_slice(&store_number.to_be_bytes());
        
        let key_len = key.len() as u32;
        request.extend_from_slice(&key_len.to_be_bytes());
        request.extend_from_slice(key);
        
        println!("DELETE REQUEST: {:?}", request);
        self.stream.write_all(&request)?;
        self.stream.flush()?;

        let response = self.read_full_response()?;
        println!("RESPONSE FOR DELETE: {:?}", response);
        let status = response[0];
        let data = &response[1..];

        match status {
            STATUS_SUCCESS => Ok(()),
            STATUS_ERROR => Err(ClientError::OperationFailed(String::from_utf8_lossy(data).into_owned())),
            _ => Err(ClientError::InvalidResponse(format!("Unexpected status: {}, response: {:?}", status, data)))
        }
    }

    pub fn write(&mut self) -> Result<(), ClientError> {
        let store_number: u8 = 0;
        let mut request = vec![OP_WRITE];
        request.extend_from_slice(&store_number.to_be_bytes());
        
        println!("WRITE REQUEST: {:?}", request);
        self.stream.write_all(&request)?;
        self.stream.flush()?;

        let response = self.read_full_response()?;
        println!("RESPONSE FOR WRITE: {:?}", response);
        let status = response[0];
        let data = &response[1..];

        match status {
            STATUS_SUCCESS => Ok(()),
            STATUS_ERROR => Err(ClientError::OperationFailed(String::from_utf8_lossy(data).into_owned())),
            _ => Err(ClientError::InvalidResponse(format!("Unexpected status: {}, response: {:?}", status, data)))
        }
    }

    pub fn check_additional_messages(&mut self) {
        println!("Checking for additional messages...");
        // Set socket to non-blocking mode for checking additional messages
        self.stream.set_nonblocking(true).unwrap_or_else(|e| println!("Failed to set non-blocking mode: {}", e));
        
        loop {
            let mut buffer = vec![0u8; 4096];
            match self.stream.read(&mut buffer) {
                Ok(n) if n > 0 => {
                    buffer.truncate(n);
                    println!("Additional message received: {:?}", buffer);
                }
                Ok(_) => {
                    println!("No more messages");
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    println!("No more messages");
                    break;
                }
                Err(e) => {
                    println!("Error reading additional messages: {}", e);
                    break;
                }
            }
        }
        
        // Set socket back to blocking mode
        self.stream.set_nonblocking(false).unwrap_or_else(|e| println!("Failed to set blocking mode: {}", e));
    }
}

#[divan::bench]
fn bench_put_operation(bencher: divan::Bencher) {
    let n: u32 = 1000;
    bencher
        .counter(divan::counter::ItemsCount::new(n))
        .bench_local(move || {
            let mut client = ScalerizeClient::connect().expect("Failed to connect");
            let store_number = 2u8;
            let key = vec![1, 2, 3, 4];
            let value = b"Hello, Scalerize!";
            client.put(store_number, &key, value).expect("Put failed");
        });
}

#[divan::bench]
fn bench_get_operation(bencher: divan::Bencher) {
    let n: u32 = 1000;
    // Setup initial data
    let mut client = ScalerizeClient::connect().expect("Failed to connect");
    let store_number = 2u8;
    let key = vec![1, 2, 3, 4];
    let value = b"Hello, Scalerize!";
    client.put(store_number, &key, value).expect("Setup put failed");
    client.write().expect("Setup write failed");

    bencher
        .counter(divan::counter::ItemsCount::new(n))
        .bench_local(move || {
            client.get(store_number, &key).expect("Get failed");
        });
}

#[divan::bench]
fn bench_write_operation(bencher: divan::Bencher) {
    let n:u32 = 1000;
    bencher
        .counter(divan::counter::ItemsCount::new(n))
        .bench_local(move || {
            let mut client = ScalerizeClient::connect().expect("Failed to connect");
            client.write().expect("Write failed");
        });
}

#[divan::bench]
fn bench_full_cycle(bencher: divan::Bencher) {
    let n: u32 = 1000;
    bencher
        .counter(divan::counter::ItemsCount::new(n))
        .bench_local(move || {
            let mut client = ScalerizeClient::connect().expect("Failed to connect");
            let store_number = 2u8;
            let key = vec![1, 2, 3, 4];
            let value = b"Hello, Scalerize!";
            
            client.put(store_number, &key, value).expect("Put failed");
            client.write().expect("Write failed");
            client.get(store_number, &key).expect("Get failed");
            client.delete(store_number, &key).expect("Delete failed");
        });
}

fn main() -> Result<(), ClientError> {
    if std::env::args().any(|arg| arg == "--bench") {
        println!("Running benchmarks...");
        divan::main();
    } else {
        // Your original main function code here
        let mut client = ScalerizeClient::connect().expect("Failed to connect");
        let store_number = 2u8;
        let key = vec![1, 2, 3, 4];
        let value: &[u8; 17] = b"Hello, Scalerize!";
        
        println!("Putting data...");
        client.put(store_number, &key, value).expect("Put failed");

        println!("Writing changes...");
        client.write().expect("Write failed");

        println!("Getting data...");
        match client.get(store_number, &key) {
            Ok(retrieved_value) => {
                println!("Operation successful!");
                println!("Retrieved value as string: {}", String::from_utf8_lossy(&retrieved_value));
            }
            Err(e) => {
                println!("Operation failed: {}", e);
            }
        }
    }

    // Ensure that we return Ok(()) at the end of the function
    Ok(())
}