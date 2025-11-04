use std::fs::{create_dir_all, read_dir, File};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;
use byteorder::{ReadBytesExt, WriteBytesExt, BigEndian};

const MAX_CONNECTIONS: usize = 10;

fn handle_client(mut stream: TcpStream) -> std::io::Result<()> {
    let command = stream.read_u8()?;
    match command {
        b'U' => handle_upload(&mut stream),
        b'D' => handle_download(&mut stream),
        b'L' => handle_list(&mut stream),
        other => {
            println!("Unknown command: {other}");
            Ok(())
        }
    }
}

fn ensure_uploads_dir() -> std::io::Result<PathBuf> {
    let uploads_dir = Path::new("uploads");
    create_dir_all(uploads_dir)?;
    uploads_dir.canonicalize()
}

fn handle_upload(stream: &mut TcpStream) -> std::io::Result<()> {
    let name_len = stream.read_u16::<BigEndian>()? as usize;
    if name_len > 4096 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("You have very big name_file, len = {}", name_len)
        ));
    }

    let mut name_buf = vec![0u8; name_len];
    stream.read_exact(&mut name_buf)?;
    let name_str = String::from_utf8(name_buf)
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid UTF-8"))?;

    let file_size = stream.read_u64::<BigEndian>()?;

    let file_name = Path::new(&name_str)
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "Bad file name"))?;

    let canonical_uploads = ensure_uploads_dir()?;
    let target_path: PathBuf = canonical_uploads.join(file_name);
    let canonical_target = target_path
        .canonicalize()
        .unwrap_or_else(|_| canonical_uploads.join(file_name));

    if !canonical_target.starts_with(&canonical_uploads) {
        stream.write_all(b"ERROR\n")?;
        return Ok(());
    }

    let mut file = File::create(&canonical_target)?;
    let mut remaining = file_size;
    let mut buffer = [0u8; 8192];
    let mut total_read = 0u64;
    let transfer_start = Instant::now();
    while remaining > 0 {
        let to_read = std::cmp::min(buffer.len() as u64, remaining) as usize;
        let n = stream.read(&mut buffer[..to_read])?;
        if n == 0 {
            break;
        }
        file.write_all(&buffer[..n])?;
        remaining -= n as u64;
        total_read += n as u64;
    }

    let actual_size = file.metadata()?.len();
    let elapsed = transfer_start.elapsed().as_secs_f64();
    let size_mb = total_read as f64 / (1024.0 * 1024.0);
    let speed = if elapsed > 0.0 { size_mb / elapsed } else { 0.0 };
    
    if actual_size != file_size {
        println!("ERROR: File size mismatch for '{}': expected {} bytes, got {} bytes", file_name, file_size, actual_size);
        drop(file);
        match std::fs::remove_file(&canonical_target) {
            Ok(_) => println!("Corrupted file '{}' has been deleted", file_name),
            Err(e) => println!("Failed to delete corrupted file '{}': {}", file_name, e),
        }
        stream.write_all(b"ERROR\n")?;
        return Ok(());
    }
    
    println!(
        "Received '{}' -> {:.2} MB in {:.3} s ({:.2} MB/s)",
        file_name,
        size_mb,
        elapsed,
        speed
    );
    stream.write_all(b"OK\n")?;
    Ok(())
}

fn handle_download(stream: &mut TcpStream) -> std::io::Result<()> {
    let name_len = stream.read_u16::<BigEndian>()? as usize;
    let mut name_buf = vec![0u8; name_len];
    stream.read_exact(&mut name_buf)?;
    let requested_name = String::from_utf8(name_buf)
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid UTF-8"))?;

    let canonical_uploads = ensure_uploads_dir()?;
    let target_path = canonical_uploads.join(&requested_name);
    let canonical_target = target_path
        .canonicalize()
        .unwrap_or_else(|_| canonical_uploads.join(&requested_name));

    if !canonical_target.starts_with(&canonical_uploads) || !canonical_target.exists() {
        stream.write_all(&[0u8])?;
        let message = b"File not found";
        stream.write_u16::<BigEndian>(message.len() as u16)?;
        stream.write_all(message)?;
        return Ok(());
    }

    let mut file = File::open(&canonical_target)?;
    let file_size = file.metadata()?.len();
    stream.write_all(&[1u8])?;
    stream.write_u64::<BigEndian>(file_size)?;

    let mut buffer = [0u8; 8192];
    let transfer_start = Instant::now();
    let mut total_written = 0u64;
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        stream.write_all(&buffer[..n])?;
        total_written += n as u64;
    }

    let elapsed = transfer_start.elapsed().as_secs_f64();
    let size_mb = total_written as f64 / (1024.0 * 1024.0);
    let speed = if elapsed > 0.0 { size_mb / elapsed } else { 0.0 };
    
    if total_written != file_size {
        println!("ERROR: Download incomplete for '{}': sent {} bytes, expected {} bytes",
                  requested_name, total_written, file_size);
        return Ok(());
    }
    
    println!(
        "Sent '{}' -> {:.2} MB in {:.3} s ({:.2} MB/s)",
        requested_name,
        size_mb,
        elapsed,
        speed
    );
    Ok(())
}

fn handle_list(stream: &mut TcpStream) -> std::io::Result<()> {
    let canonical_uploads = ensure_uploads_dir()?;
    let mut entries: Vec<(String, u64)> = Vec::new();
    for entry in read_dir(&canonical_uploads)? {
        if let Ok(entry) = entry {
            if entry.file_type()?.is_file() {
                if let Some(name) = entry.file_name().to_str() {
                    let size = entry.metadata()?.len();
                    entries.push((name.to_string(), size));
                }
            }
        }
    }
    stream.write_u16::<BigEndian>(entries.len() as u16)?;
    for (name, size) in entries {
        let bytes = name.as_bytes();
        stream.write_u16::<BigEndian>(bytes.len() as u16)?;
        stream.write_all(bytes)?;
        stream.write_u64::<BigEndian>(size)?;
    }
    Ok(())
}

fn main() -> std::io::Result<()> {
    let active_connections = Arc::new(Mutex::new(0usize));
    
    let listener = TcpListener::bind("127.0.0.1:4000")?;
    println!("Listening on port 5000...");
    println!("Max concurrent connections: {}", MAX_CONNECTIONS);
    
    for stream in listener.incoming() {
        match stream {
            Ok(s) => {
                let mut count = active_connections.lock().unwrap();
                
                if *count >= MAX_CONNECTIONS {
                    println!("Connection rejected: max limit ({}) reached", MAX_CONNECTIONS);
                    drop(count);
                    drop(s);
                    continue;
                }
                
                *count += 1;
                drop(count);
                
                println!("Client connected. id {}", &s.peer_addr().unwrap());
                
                let counter = Arc::clone(&active_connections);
                thread::spawn(move || {
                    let ip = s.peer_addr().unwrap();
                    if let Err(e) = handle_client(s) {
                        println!("Client error: {:?}", e);
                    }
                    
                    let mut count = counter.lock().unwrap();
                    *count -= 1;
                    println!("Client disconnected. id {}", ip);
                });
            }
            Err(e) => println!("Connection failed: {:?}", e),
        }
    }
    Ok(())
}
