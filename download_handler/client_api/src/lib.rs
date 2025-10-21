use std::fs::File;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::time::Instant;
use byteorder::{ReadBytesExt, WriteBytesExt, BigEndian};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct RemoteFileInfo {
    pub name: String,
    pub size_mb: f64,
}

pub fn fetch_available_files(server_addr: &str) -> std::io::Result<Vec<RemoteFileInfo>> {
    let mut stream = TcpStream::connect(server_addr)?;
    stream.write_all(&[b'L'])?;

    let count = stream.read_u16::<BigEndian>()? as usize;
    let mut files = Vec::with_capacity(count);
    for _ in 0..count {
        let name_len = stream.read_u16::<BigEndian>()? as usize;
        let mut buf = vec![0u8; name_len];
        stream.read_exact(&mut buf)?;
        let name = String::from_utf8(buf).unwrap_or_default();
        let size_bytes = stream.read_u64::<BigEndian>()?;
        if !name.is_empty() {
            let size_mb = size_bytes as f64 / (1024.0 * 1024.0);
            files.push(RemoteFileInfo { name, size_mb });
        }
    }
    Ok(files)
}

pub fn upload_file(path: &Path, server_addr: &str) -> std::io::Result<()> {
    let mut file = File::open(path)?;
    let mut content = Vec::new();
    file.read_to_end(&mut content)?;

    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "Bad file name"))?
        .as_bytes()
        .to_owned();

    let mut stream = TcpStream::connect(server_addr)?;
    stream.write_all(&[b'U'])?;

    let transfer_start = Instant::now();
    stream.write_u16::<BigEndian>(file_name.len() as u16)?;
    stream.write_all(&file_name)?;
    stream.write_u64::<BigEndian>(content.len() as u64)?;
    stream.write_all(&content)?;

    let elapsed = transfer_start.elapsed().as_secs_f64();
    let size_mb = content.len() as f64 / (1024.0 * 1024.0);
    let speed = if elapsed > 0.0 { size_mb / elapsed } else { 0.0 };
    println!(
        "Upload speed: {:.2} MB in {:.3} s ({:.2} MB/s)",
        size_mb,
        elapsed,
        speed
    );

    let mut resp = String::new();
    stream.read_to_string(&mut resp)?;
    println!("Server response: {}", resp.trim());
    Ok(())
}

pub fn download_file(file_name: &str, destination: &Path, server_addr: &str) -> std::io::Result<()> {
    let mut stream = TcpStream::connect(server_addr)?;
    stream.write_all(&[b'D'])?;

    let name_bytes = file_name.as_bytes();
    stream.write_u16::<BigEndian>(name_bytes.len() as u16)?;
    stream.write_all(name_bytes)?;

    let status = stream.read_u8()?;
    if status == 0 {
        let msg_len = stream.read_u16::<BigEndian>()? as usize;
        let mut buf = vec![0u8; msg_len];
        stream.read_exact(&mut buf)?;
        let message = String::from_utf8(buf).unwrap_or_else(|_| "Unknown error".to_string());
        println!("Download failed: {}", message);
        return Ok(());
    }

    let size = stream.read_u64::<BigEndian>()?;
    let mut file = File::create(destination)?;
    let mut remaining = size;
    let mut buffer = [0u8; 8192];
    let transfer_start = Instant::now();
    while remaining > 0 {
        let to_read = std::cmp::min(buffer.len() as u64, remaining) as usize;
        let n = stream.read(&mut buffer[..to_read])?;
        if n == 0 {
            break;
        }
        file.write_all(&buffer[..n])?;
        remaining -= n as u64;
    }

    let elapsed = transfer_start.elapsed().as_secs_f64();
    let size_mb = size as f64 / (1024.0 * 1024.0);
    let speed = if elapsed > 0.0 { size_mb / elapsed } else { 0.0 };
    println!(
        "Downloaded {:.2} MB to {:?} in {:.3} s ({:.2} MB/s)",
        size_mb,
        destination,
        elapsed,
        speed
    );
    Ok(())
}
