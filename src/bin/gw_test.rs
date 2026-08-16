use std::env;
use std::time::Duration;

fn send_command(
    port: &mut Box<dyn serialport::SerialPort>,
    cmd_name: &str,
    cmd: &[u8],
    extra_read_bytes: usize,
) {
    println!("Sending {}", cmd_name);
    if let Err(e) = port.write_all(cmd) {
        println!("  -> Write error {} : {}", cmd_name, e);
        return;
    }
    if let Err(e) = port.flush() {
        println!("  -> Flush error {} : {}", cmd_name, e);
        return;
    }

    // 2 bytes ACK : [CMD_ECHO, STATUS]
    let mut ack = [0u8; 2];
    match port.read_exact(&mut ack) {
        Ok(_) => {
            let status_str = match ack[1] {
                0 => "ACK_OKAY (0)",
                1 => "ACK_BAD_COMMAND (1)",
                2 => "ACK_NO_INDEX (2)",
                3 => "ACK_NO_TRK0 (3)",
                7 => "ACK_NO_UNIT (7)",
                8 => "ACK_NO_BUS (8)",
                9 => "ACK_BAD_UNIT (9)",
                10 => "ACK_BAD_PIN (10)",
                11 => "ACK_BAD_CYLINDER (11)",
                _other => "OTHER_STATUS",
            };
            println!(
                "  -> Response {} : CMD_ECHO={}, STATUS={}",
                cmd_name, ack[0], status_str
            );

            if ack[1] == 0 && extra_read_bytes > 0 {
                let mut buf = vec![0u8; extra_read_bytes];
                if let Err(e) = port.read_exact(&mut buf) {
                    println!("  -> Extra data read error: {}", e);
                } else {
                    println!("  -> Extra data ({} bytes) received.", extra_read_bytes);
                }
            }
        }
        Err(e) => println!("  -> Read error {} : {}", cmd_name, e),
    }
    std::thread::sleep(std::time::Duration::from_millis(50));
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <port_name> [unit_id]", args[0]);
        eprintln!("Examples:");
        eprintln!("  {} COM2 0", args[0]);
        eprintln!("  {} COM2 1", args[0]);
        std::process::exit(1);
    }

    let port_name = &args[1];
    let unit: u8 = if args.len() >= 3 {
        args[2].parse().unwrap_or(0)
    } else {
        0
    };

    println!("Connecting to serial port : {}", port_name);
    println!("Target Unit (Drive Select) : {}", unit);

    let mut port = serialport::new(port_name, 115_200)
        .timeout(Duration::from_secs(5))
        .open()?;

    println!("Port opened successfully.");

    // Activate DTR and RTS
    if let Err(e) = port.write_data_terminal_ready(true) {
        eprintln!("Warning: Failed to set DTR: {}", e);
    }
    if let Err(e) = port.write_request_to_send(true) {
        eprintln!("Warning: Failed to set RTS: {}", e);
    }

    // CMD_GET_INFO (0) : [0x00, 0x03, 0x00] -> returns 2 bytes ACK + 32 bytes payload
    send_command(&mut port, "GET_INFO", &[0x00, 0x03, 0x00], 32);

    // CMD_SET_BUS_TYPE (14 / 0x0E) : [0x0E, 0x03, 0x01] (CMD=14, LEN=3, TYPE=1 IBMPC)
    send_command(&mut port, "SET_BUS_TYPE (IBMPC)", &[0x0E, 0x03, 0x01], 0);

    // CMD_SELECT (12 / 0x0C) : [0x0C, 0x03, unit] (CMD=12, LEN=3, UNIT=0 or 1)
    send_command(
        &mut port,
        &format!("SELECT_UNIT (Drive {})", unit),
        &[0x0C, 0x03, unit],
        0,
    );

    // CMD_MOTOR (6 / 0x06) : [0x06, 0x04, unit, 0x01] (CMD=6, LEN=4, UNIT, STATE=1 ON)
    send_command(
        &mut port,
        &format!("SET_MOTOR ON (Drive {})", unit),
        &[0x06, 0x04, unit, 0x01],
        0,
    );

    // Pause 2 seconds for motor spin up
    println!("Pause 2 seconds (Motor ON)...");
    std::thread::sleep(Duration::from_secs(2));

    // CMD_SEEK (2 / 0x02) : [0x02, 0x03, 0x00] (CMD=2, LEN=3, CYL=0)
    send_command(&mut port, "SEEK Track 0", &[0x02, 0x03, 0x00], 0);

    // Pause 1 sec
    std::thread::sleep(Duration::from_secs(1));

    // CMD_SEEK (2 / 0x02) : [0x02, 0x03, 0x0A] (CMD=2, LEN=3, CYL=10)
    send_command(&mut port, "SEEK Track 10", &[0x02, 0x03, 0x0A], 0);

    // Pause 1 sec
    std::thread::sleep(Duration::from_secs(1));

    // CMD_SEEK (2 / 0x02) : [0x02, 0x03, 0x00] (CMD=2, LEN=3, CYL=0)
    send_command(&mut port, "SEEK Track 0", &[0x02, 0x03, 0x00], 0);

    // Pause 1 second
    println!("Pause 1 second...");
    std::thread::sleep(Duration::from_secs(1));

    // CMD_MOTOR (6 / 0x06) : [0x06, 0x04, unit, 0x00] (CMD=6, LEN=4, UNIT, STATE=0 OFF)
    send_command(
        &mut port,
        &format!("SET_MOTOR OFF (Drive {})", unit),
        &[0x06, 0x04, unit, 0x00],
        0,
    );

    Ok(())
}
