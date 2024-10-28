use std::io::Cursor;
use std::time::Instant;
use rayon::prelude::*;
// use serde_json::Value;
use sonic_rs::Value;
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use mockito::Server;
use reqwest::blocking::multipart::{Form, Part};
use uuid::Uuid;

fn create_json_with_large_array(len: usize) -> Value {
    let large_array: Vec<Value> = (0..len)
        .map(|i| {
            sonic_rs::json!({
                "index": i,
                "data": format!("This is element number {}", i),
                "nested": {
                    "id": i,
                    "value": format!("Nested value for element {}", i),
                }
            })
        })
        .collect();

    sonic_rs::json!({
        "name": "Huge JSON",
        "description": "This is a very large JSON object for benchmarking purposes.",
        "array": large_array,
        "metadata": {
            "created_at": "2024-10-22T19:00:00Z",
            "author": "Rust Program",
            "version": 1.0
        }
    })
}

fn create_json_with_large_strings(len: usize) -> Value {
    let large_string = "a".repeat(len);
    sonic_rs::json!({
        "name": "Huge JSON",
        "description": "This is a very large JSON object for benchmarking purposes.",
        "key1": large_string.clone(),
        "key2": large_string.clone(),
        "key3": large_string.clone(),
        "metadata": {
            "created_at": "2024-10-22T19:00:00Z",
            "author": "Rust Program",
            "version": 1.0
        }
    })
}

// Sequential processing
fn benchmark_sequential(data: &[Value]) -> Vec<Vec<u8>> {
    data.iter()
        .map(|json| sonic_rs::to_vec(json).expect("Failed to serialize JSON"))
        .collect()
}

// Parallel processing
fn benchmark_parallel(data: &[Value]) -> Vec<Vec<u8>> {
    data.par_iter()
        .map(|json| sonic_rs::to_vec(json).expect("Failed to serialize JSON"))
        .collect()
}

// into par iter
fn benchmark_into_par_iter(data: &[Value]) -> Vec<Vec<u8>> {
    for _ in 0..4 {
        let start = Instant::now();
        let _ = data.into_par_iter()
            .map(|json| sonic_rs::to_vec(json).expect("Failed to serialize JSON"))
            .collect::<Vec<_>>();
        println!("into_par_iter took {:?}", start.elapsed());
    }
    data.into_par_iter()
        .map(|json| sonic_rs::to_vec(&json).expect("Failed to serialize JSON"))
        .collect()
}

fn json_benchmark_large_array(c: &mut Criterion) {
    let num_json_objects = 300;
    let json_length = 3000;
    let data: Vec<Value> = (0..num_json_objects)
        .map(|_| create_json_with_large_array(json_length))
        .collect();

    let mut group = c.benchmark_group("json_benchmark_large_array");
    group.bench_function("sequential serialization", |b|
        b.iter_with_large_drop(|| benchmark_sequential(&data))
    );
    group.bench_function("parallel serialization", |b|
        b.iter_with_large_drop(|| benchmark_parallel(&data))
    );
    group.bench_function("into par iter serialization", |b|
        b.iter_with_large_drop(|| benchmark_into_par_iter(&data))
    );
}

fn json_benchmark_large_strings(c: &mut Criterion) {
    let num_json_objects = 100;
    let json_length = 100_000;
    let data: Vec<Value> = (0..num_json_objects)
        .map(|_| create_json_with_large_strings(json_length))
        .collect();

    let mut group = c.benchmark_group("json_benchmark_large_strings");
    group.bench_function("sequential serialization", |b|
        b.iter_with_large_drop(|| benchmark_sequential(&data))
    );
    group.bench_function("parallel serialization", |b|
        b.iter_with_large_drop(|| benchmark_parallel(&data))
    );
    group.bench_function("into par iter serialization", |b|
        b.iter_with_large_drop(|| benchmark_into_par_iter(&data))
    );
}

fn hitting_mock_server_benchmark(c: &mut Criterion) {
    let server = {
        let mut server = Server::new();
        server
            .mock("POST", "/runs/multipart")
            .with_status(202)
            .create();
        server
    };

    let mut group = c.benchmark_group("hitting_mock_server_benchmark");
    let reqwest = reqwest::blocking::Client::new();
    group.bench_function("hitting mock server with reqwest", |b| {
        b.iter_custom(|iters| {

            let num_json_objects = 300;
            let json_length = 3000;
            let data: Vec<Value> = (0..num_json_objects)
                .map(|_| create_json_with_large_array(json_length))
                .collect();

            let bytes: Vec<Part> = data.par_iter()
                .map(|json| {
                    let data = sonic_rs::to_vec(json).expect("Failed to serialize JSON");
                    Part::bytes(data)
                        .file_name("part".to_string())
                        .mime_str("application/json")
                        .unwrap()
                })
                .collect();

            let mut form = Form::new();
            for (i, part) in bytes.into_iter().enumerate() {
                let part_name = format!("part{}", i);
                form = form.part(part_name, part);
            }

            let start = Instant::now();
            let response = reqwest
                .post(format!("{}/runs/multipart", server.url()))
                .multipart(form)
                .send()
                .unwrap();
            assert_eq!(response.status(), 202);
            start.elapsed()
        });
    });

    // now let's try ureq
    let ureq = ureq::Agent::new();
    group.bench_function("hitting mock server with ureq", |b| {
        b.iter_custom(|iters| {
            let num_json_objects = 300;
            let json_length = 3000;
            let data: Vec<Value> = (0..num_json_objects)
                .map(|_| create_json_with_large_array(json_length))
                .collect();

            let bytes: Vec<Vec<u8>> = data.par_iter()
                .map(|json| {
                    let data = sonic_rs::to_vec(json).expect("Failed to serialize JSON");
                    data
                })
                .collect();

            let mut multipart_body = Vec::new();
            let boundary = format!("------------------------{}", Uuid::new_v4());

            for (i, data_bytes) in bytes.iter().enumerate() {
                multipart_body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
                multipart_body.extend_from_slice(
                    format!(
                        "Content-Disposition: form-data; name=\"part{}\"\r\n",
                        i
                    )
                        .as_bytes(),
                );
                multipart_body.extend_from_slice(b"Content-Type: application/json\r\n\r\n");
                multipart_body.extend_from_slice(data_bytes);
                multipart_body.extend_from_slice(b"\r\n");
            }
            multipart_body.extend_from_slice(format!("--{}--\r\n", boundary).as_bytes());

            // Convert the multipart body to a Cursor for reading
            let body_reader = Cursor::new(multipart_body);

            // Send the request
            let start = Instant::now();
            let response = ureq
                .post(&format!("{}/runs/multipart", server.url()))
                .set(
                    "Content-Type",
                    &format!("multipart/form-data; boundary={}", boundary),
                )
                .send(body_reader);

            assert_eq!(response.unwrap().status(), 202);
            start.elapsed()
        });
    });
}

// criterion_group! {
//     name = benches;
//     config = Criterion::default().sample_size(10);
//     targets = hitting_mock_server_benchmark,
// }
// criterion_main!(benches);

fn main() {
}
