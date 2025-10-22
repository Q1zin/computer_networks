use std::fs::File;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::time::{Instant, Duration};
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

pub fn upload_file<F>(path: &Path, server_addr: &str, mut on_progress: F) -> std::io::Result<()>
where
    F: FnMut(f64, f64, f64),
{
    let mut file = File::open(path)?;
    let metadata = file.metadata()?;
    let total_size = metadata.len();

    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "Bad file name"))?
        .as_bytes()
        .to_owned();

    let mut stream = TcpStream::connect(server_addr)?;
    stream.write_all(&[b'U'])?;

    stream.write_u16::<BigEndian>(file_name.len() as u16)?;
    stream.write_all(&file_name)?;
    stream.write_u64::<BigEndian>(total_size)?;

    let mut sent_bytes: u64 = 0;
    let mut buffer = [0u8; 8192];
    let start_time = Instant::now();
    let mut last_time = Instant::now();
    let mut last_sent: u64 = 0;

    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        stream.write_all(&buffer[..n])?;
        sent_bytes += n as u64;

        let now = Instant::now();
        let elapsed_since_last = now.duration_since(last_time);

        if elapsed_since_last >= Duration::from_millis(200) {
            let progress = (sent_bytes as f64 / total_size as f64) * 100.0;
            let total_elapsed = now.duration_since(start_time).as_secs_f64();

            let delta_bytes = sent_bytes - last_sent;
            let instant_speed = (delta_bytes as f64 / (1024.0 * 1024.0)) / elapsed_since_last.as_secs_f64();

            let avg_speed = (sent_bytes as f64 / (1024.0 * 1024.0)) / total_elapsed;

            on_progress(progress, instant_speed, avg_speed);

            last_sent = sent_bytes;
            last_time = now;
        }
    }

    let total_elapsed = start_time.elapsed().as_secs_f64();
    let avg_speed = (total_size as f64 / (1024.0 * 1024.0)) / total_elapsed;
    on_progress(100.0, 0.0, avg_speed);

    if sent_bytes != total_size {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Upload incomplete: sent {} bytes, expected {} bytes", sent_bytes, total_size)
        ));
    }

    let mut resp = String::new();
    stream.read_to_string(&mut resp)?;
    
    if !resp.trim().starts_with("OK") {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Server rejected file: {}", resp.trim())
        ));
    }

    Ok(())
}

pub fn download_file<F>(file_name: &str, destination: &Path, server_addr: &str, mut on_progress: F) -> std::io::Result<()> 
where
    F: FnMut(f64, f64, f64),
{
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
        return Err(std::io::Error::new(std::io::ErrorKind::Other, message));
    }

    let total_size = stream.read_u64::<BigEndian>()?;
    let mut file = File::create(destination)?;

    let mut received: u64 = 0;
    let mut buffer = [0u8; 8192];
    let start_time = Instant::now();
    let mut last_time = Instant::now();
    let mut last_received: u64 = 0;

    loop {
        let n = stream.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        file.write_all(&buffer[..n])?;
        received += n as u64;

        let now = Instant::now();
        let elapsed_since_last = now.duration_since(last_time);

        if elapsed_since_last.as_millis() > 150 {
            let progress = (received as f64 / total_size as f64) * 100.0;
            let total_elapsed = now.duration_since(start_time).as_secs_f64().max(1e-6);

            let delta = received - last_received;
            let instant = (delta as f64 / (1024.0 * 1024.0)) / elapsed_since_last.as_secs_f64();
            let avg = (received as f64 / (1024.0 * 1024.0)) / total_elapsed;

            on_progress(progress, instant, avg);

            last_received = received;
            last_time = now;
        }

        if received >= total_size {
            break;
        }
    }

    let total_elapsed = start_time.elapsed().as_secs_f64().max(1e-6);
    let avg_speed = (received as f64 / (1024.0 * 1024.0)) / total_elapsed;
    on_progress(100.0, 0.0, avg_speed);

    if received != total_size {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            format!("Download incomplete: received {} bytes, expected {} bytes", received, total_size)
        ));
    }

    Ok(())
}
