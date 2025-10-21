use client_api::*;

fn main() {
    let server_addr = "127.0.0.1:4000";
    let available_files = fetch_available_files(server_addr);
    println!("{:?}", available_files);
}
