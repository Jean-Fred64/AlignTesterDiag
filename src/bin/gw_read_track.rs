use serialport::SerialPort;
use std::{env, io::Write, time::Duration};

fn gw_send(
    port: &mut Box<dyn SerialPort>,
    cmd: &[u8],
    extra_read: usize,
) -> Result<(u8, Vec<u8>), Box<dyn std::error::Error>> {
    port.write_all(cmd)?;
    port.flush()?;
    let mut ack = [0u8; 2];
    port.read_exact(&mut ack)?;

    let mut extra = Vec::new();
    if ack[1] == 0 && extra_read > 0 {
        extra.resize(extra_read, 0u8);
        port.read_exact(&mut extra)?;
    }

    Ok((ack[1], extra))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    let port_name = args.get(1).map(|s| s.as_str()).unwrap_or("COM2");
    let cyl: u8 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);

    println!("=== GREASEWEAZLE FLUX CELL DURATION MEASUREMENT TEST ===");
    println!("Port : {}, Track : {}", port_name, cyl);

    let mut port = serialport::new(port_name, 115_200)
        .timeout(Duration::from_secs(5))
        .open()?;

    let _ = port.write_data_terminal_ready(true);
    let _ = port.write_request_to_send(true);

    let _ = gw_send(&mut port, &[0x00, 0x03, 0x00], 32)?;
    let _ = gw_send(&mut port, &[0x0E, 0x03, 0x01], 0)?;
    let _ = gw_send(&mut port, &[0x0C, 0x03, 0x00], 0)?;
    let _ = gw_send(&mut port, &[0x06, 0x04, 0x00, 0x01], 0)?;
    std::thread::sleep(Duration::from_millis(500));

    let _ = gw_send(&mut port, &[0x02, 0x03, cyl], 0)?;

    // Read flux 1 revolution
    let read_cmd = [0x07, 0x0A, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00];
    port.write_all(&read_cmd)?;
    port.flush()?;

    let mut ack = [0u8; 2];
    port.read_exact(&mut ack)?;

    let mut stream_bytes = Vec::new();
    if ack[1] == 0 {
        let start = std::time::Instant::now();
        let mut buf = [0u8; 1024];
        while start.elapsed() < Duration::from_millis(300) && stream_bytes.len() < 65536 {
            if let Ok(n) = port.read(&mut buf) {
                if n > 0 {
                    stream_bytes.extend_from_slice(&buf[..n]);
                }
            }
        }
    }

    println!("Captured raw flux size: {} bytes", stream_bytes.len());

    let mut total_ticks: u64 = 0;
    let mut transition_count: u64 = 0;
    let mut space_ticks: u64 = 0;
    let mut sample_preview = Vec::new();

    for &b in &stream_bytes {
        if (1..=249).contains(&b) {
            let val = space_ticks + (b as u64);
            total_ticks += val;
            if sample_preview.len() < 20 {
                sample_preview.push(val);
            }
            space_ticks = 0;
            transition_count += 1;
        } else if b == 0xFA {
            space_ticks += 250;
        } else {
            space_ticks = 0;
        }
    }

    if let Some(avg_cell_ticks) = total_ticks.checked_div(transition_count) {
        println!("Captured transitions count : {}", transition_count);
        println!("Average flux cell duration : {} ticks", avg_cell_ticks);

        // Exact threshold : 240 ticks
        // HD (500k) ~150-180 ticks | DD (250k) ~300-360 ticks
        if avg_cell_ticks > 240 {
            println!("==> DECISION : 250k DD (720K - 9 Sectors)");
        } else {
            println!("==> DECISION : 500k HD (1.44M - 18 Sectors)");
        }
    } else {
        println!("No flux transitions measured.");
    }

    let _ = gw_send(&mut port, &[0x06, 0x04, 0x00, 0x00], 0)?;
    println!("Motor OFF.");

    Ok(())
}
