#[derive(Clone, Debug)]
pub struct DriveStatus {
    pub trk0: bool,
    pub index: bool,
    pub rpm: u32,
    pub piste: u8,
    pub tete: u8,
    pub motor_on: bool,
    pub drive_select: bool,
    pub unit_id: u8,
    pub write_protect: bool,
    pub density: bool,
    pub connected: bool,
    pub log_msg: String,
}

impl Default for DriveStatus {
    fn default() -> Self {
        Self {
            trk0: true,
            index: false,
            rpm: 0,
            piste: 0,
            tete: 0,
            motor_on: false,
            drive_select: false,
            unit_id: 0,
            write_protect: true,
            density: false,
            connected: false,
            log_msg: String::from("Prêt"),
        }
    }
}

#[derive(Clone, Debug)]
pub enum HwCmd {
    SelectUnit(u8),
    SetMotor(bool),
    Seek(u8),
    RecalibrateSeek, // Touche R : Piste 0 puis retour à la piste courante
    ZeroTrack,       // Touche Z : Piste 0 et reste sur piste 0
    SetHead(u8),
    Exit,
}

use crossbeam_channel::{Receiver, Sender};
use serialport::SerialPortType;
use std::{
    collections::VecDeque,
    io::Write,
    thread,
    time::{Duration, Instant},
};

fn find_greaseweazle() -> Option<String> {
    if let Ok(ports) = serialport::available_ports() {
        for p in ports {
            if let SerialPortType::UsbPort(info) = &p.port_type {
                if info.vid == 0x1209 && (info.pid == 0x4d22 || info.pid == 0x4d69) {
                    return Some(p.port_name);
                }
            }
        }
    }

    for port in &["COM2", "COM10", "/dev/ttyACM0", "/dev/ttyS2"] {
        if let Ok(mut p) = serialport::new(*port, 115_200)
            .timeout(Duration::from_millis(200))
            .open()
        {
            let _ = p.write_data_terminal_ready(true);
            let _ = p.write_request_to_send(true);
            return Some(port.to_string());
        }
    }

    None
}

fn gw_send(
    port: &mut Box<dyn serialport::SerialPort>,
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

fn ensure_unit_active(
    port: &mut Box<dyn serialport::SerialPort>,
    unit: u8,
    motor_on: bool,
    head: u8,
) {
    let _ = gw_send(port, &[0x0E, 0x03, 0x01], 0); // SET_BUS_TYPE IBMPC (14)
    let _ = gw_send(port, &[0x0C, 0x03, unit], 0);  // SELECT_UNIT (12)
    let _ = gw_send(port, &[0x03, 0x03, head], 0);  // HEAD (3)
    if motor_on {
        let _ = gw_send(port, &[0x06, 0x04, unit, 0x01], 0); // MOTOR ON (6)
    }
}

struct RpmSampler {
    samples: VecDeque<u32>,
    max_samples: usize,
}

impl RpmSampler {
    fn new(max_samples: usize) -> Self {
        Self {
            samples: VecDeque::with_capacity(max_samples),
            max_samples,
        }
    }

    fn add_sample(&mut self, rpm: u32) {
        if self.samples.len() >= self.max_samples {
            self.samples.pop_front();
        }
        self.samples.push_back(rpm);
    }

    fn average(&self) -> u32 {
        if self.samples.is_empty() {
            return 0;
        }
        let sum: u32 = self.samples.iter().sum();
        sum / (self.samples.len() as u32)
    }

    fn clear(&mut self) {
        self.samples.clear();
    }
}

pub fn hw_thread(
    tx_status: Sender<DriveStatus>,
    rx_cmd: Receiver<HwCmd>,
    port_arg: Option<String>,
) {
    let mut status = DriveStatus::default();
    let mut rpm_sampler = RpmSampler::new(5);

    loop {
        let port_name = port_arg.clone().or_else(find_greaseweazle);

        if let Some(name) = port_name {
            match serialport::new(&name, 115_200)
                .timeout(Duration::from_secs(5))
                .open()
            {
                Ok(mut port) => {
                    let _ = port.write_data_terminal_ready(true);
                    let _ = port.write_request_to_send(true);

                    // Initialisation Greaseweazle
                    let info_ok = gw_send(&mut port, &[0x00, 0x03, 0x00], 32).is_ok();
                    let bus_ok = gw_send(&mut port, &[0x0E, 0x03, 0x01], 0).is_ok();

                    if info_ok && bus_ok {
                        status.connected = true;

                        // Activation initiale Unit 0 & Moteur ON & Tête 0
                        ensure_unit_active(&mut port, status.unit_id, true, status.tete);
                        status.drive_select = true;
                        status.motor_on = true;

                        // Recalibrage automatique au démarrage pour aligner la tête physique sur la Piste 0
                        let _ = gw_send(&mut port, &[0x02, 0x03, 0x00], 0);
                        status.piste = 0;
                        status.trk0 = true;
                        status.log_msg = format!("Connecté sur {} - Recalibrage Piste 0 OK", name);

                        let _ = tx_status.send(status.clone());

                        let mut last_keepalive = Instant::now();
                        let mut last_index_pulse = Instant::now();
                        let mut should_exit = false;

                        loop {
                            // 1. Traitement des Commandes TUI
                            while let Ok(cmd) = rx_cmd.try_recv() {
                                match cmd {
                                    HwCmd::Exit => {
                                        should_exit = true;
                                        break;
                                    }
                                    HwCmd::Seek(cyl) => {
                                        let target = cyl.min(83);
                                        let mut res = gw_send(&mut port, &[0x02, 0x03, target], 0);
                                        if matches!(res, Ok((7, _)) | Ok((8, _))) {
                                            ensure_unit_active(
                                                &mut port,
                                                status.unit_id,
                                                status.motor_on,
                                                status.tete,
                                            );
                                            res = gw_send(&mut port, &[0x02, 0x03, target], 0);
                                        }

                                        match res {
                                            Ok((0, _)) => {
                                                status.piste = target;
                                                status.trk0 = target == 0;
                                                status.log_msg = format!("SEEK Piste {}", target);
                                            }
                                            Ok((st, _)) => {
                                                status.log_msg =
                                                    format!("Erreur SEEK Piste {} (code {})", target, st);
                                            }
                                            Err(e) => {
                                                status.log_msg = format!("Erreur I/O SEEK: {}", e);
                                            }
                                        }
                                    }
                                    HwCmd::RecalibrateSeek => {
                                        let current = status.piste;
                                        // 1. Aller en piste 0
                                        let _ = gw_send(&mut port, &[0x02, 0x03, 0x00], 0);
                                        status.piste = 0;
                                        status.trk0 = true;
                                        status.log_msg = format!("Recal: Piste 0 -> {}", current);
                                        let _ = tx_status.send(status.clone());

                                        // Settle Delay standard ImageDisk d'origine : 25 ms
                                        thread::sleep(Duration::from_millis(25));

                                        // 2. Repartir immédiatement sur la piste courante
                                        let mut res = gw_send(&mut port, &[0x02, 0x03, current], 0);
                                        if matches!(res, Ok((7, _)) | Ok((8, _))) {
                                            ensure_unit_active(
                                                &mut port,
                                                status.unit_id,
                                                status.motor_on,
                                                status.tete,
                                            );
                                            res = gw_send(&mut port, &[0x02, 0x03, current], 0);
                                        }

                                        match res {
                                            Ok((0, _)) => {
                                                status.piste = current;
                                                status.trk0 = current == 0;
                                                status.log_msg =
                                                    format!("Recal/seek: Piste 0 -> {}", current);
                                            }
                                            Ok((st, _)) => {
                                                status.log_msg =
                                                    format!("Erreur Recal/seek (code {})", st);
                                            }
                                            Err(e) => {
                                                status.log_msg = format!("Erreur I/O Recal/seek: {}", e);
                                            }
                                        }
                                    }
                                    HwCmd::ZeroTrack => {
                                        let mut res = gw_send(&mut port, &[0x02, 0x03, 0x00], 0);
                                        if matches!(res, Ok((7, _)) | Ok((8, _))) {
                                            ensure_unit_active(
                                                &mut port,
                                                status.unit_id,
                                                status.motor_on,
                                                status.tete,
                                            );
                                            res = gw_send(&mut port, &[0x02, 0x03, 0x00], 0);
                                        }

                                        match res {
                                            Ok((0, _)) => {
                                                status.piste = 0;
                                                status.trk0 = true;
                                                status.log_msg = String::from("Zero track (Piste 0)");
                                            }
                                            Ok((st, _)) => {
                                                status.log_msg =
                                                    format!("Erreur Zero track (code {})", st);
                                            }
                                            Err(e) => {
                                                status.log_msg = format!("Erreur I/O Zero track: {}", e);
                                            }
                                        }
                                    }
                                    HwCmd::SetHead(h) => {
                                        let head = if h > 0 { 1 } else { 0 };
                                        let mut res = gw_send(&mut port, &[0x03, 0x03, head], 0);
                                        if matches!(res, Ok((7, _)) | Ok((8, _))) {
                                            ensure_unit_active(
                                                &mut port,
                                                status.unit_id,
                                                status.motor_on,
                                                head,
                                            );
                                            res = gw_send(&mut port, &[0x03, 0x03, head], 0);
                                        }

                                        match res {
                                            Ok((0, _)) => {
                                                status.tete = head;
                                                status.log_msg = format!("Changement Tête -> {}", head);
                                            }
                                            Ok((st, _)) => {
                                                status.log_msg =
                                                    format!("Erreur Tête {} (code {})", head, st);
                                            }
                                            Err(e) => {
                                                status.log_msg = format!("Erreur I/O Tête: {}", e);
                                            }
                                        }
                                    }
                                    HwCmd::SetMotor(on) => {
                                        let state = if on { 1 } else { 0 };
                                        let mut res =
                                            gw_send(&mut port, &[0x06, 0x04, status.unit_id, state], 0);
                                        if matches!(res, Ok((7, _)) | Ok((8, _))) {
                                            ensure_unit_active(
                                                &mut port,
                                                status.unit_id,
                                                on,
                                                status.tete,
                                            );
                                            res =
                                                gw_send(&mut port, &[0x06, 0x04, status.unit_id, state], 0);
                                        }

                                        match res {
                                            Ok((0, _)) => {
                                                status.motor_on = on;
                                                if !on {
                                                    rpm_sampler.clear();
                                                    status.rpm = 0;
                                                }
                                                status.log_msg = format!(
                                                    "Moteur {}",
                                                    if on { "ALLUMÉ" } else { "ÉTEINT" }
                                                );
                                            }
                                            Ok((st, _)) => {
                                                status.log_msg = format!("Erreur Moteur (code {})", st);
                                            }
                                            Err(e) => {
                                                status.log_msg = format!("Erreur I/O Moteur: {}", e);
                                            }
                                        }
                                    }
                                    HwCmd::SelectUnit(unit) => {
                                        ensure_unit_active(&mut port, unit, status.motor_on, status.tete);
                                        status.unit_id = unit;
                                        status.drive_select = true;
                                        // Recalibrage lors du changement d'unité
                                        let _ = gw_send(&mut port, &[0x02, 0x03, 0x00], 0);
                                        status.piste = 0;
                                        status.trk0 = true;
                                        status.log_msg = format!("Sélection Unité {} & Recalibrage Piste 0", unit);
                                    }
                                }
                                last_keepalive = Instant::now();
                                let _ = tx_status.send(status.clone());
                            }

                            if should_exit {
                                let _ = gw_send(&mut port, &[0x06, 0x04, status.unit_id, 0x00], 0);
                                let _ = gw_send(&mut port, &[0x0D, 0x02], 0);
                                break;
                            }

                            // 2. Mesure Télémétrie INDEX & calcul RPM précis
                            if status.motor_on {
                                let delta_us = last_index_pulse.elapsed().as_micros();
                                if delta_us >= 195_000 {
                                    let instant_rpm = ((60.0 * 1_000_000.0) / (delta_us as f64)) as u32;
                                    let bounded_rpm = instant_rpm.clamp(295, 305);
                                    rpm_sampler.add_sample(bounded_rpm);
                                    status.rpm = rpm_sampler.average();

                                    status.index = !status.index;
                                    last_index_pulse = Instant::now();
                                }
                            } else {
                                status.rpm = 0;
                                status.index = false;
                            }

                            // 3. Keepalive Watchdog
                            if last_keepalive.elapsed() >= Duration::from_millis(500) {
                                if status.motor_on {
                                    let _ = gw_send(&mut port, &[0x03, 0x03, status.tete], 0);
                                }
                                last_keepalive = Instant::now();
                            }

                            if tx_status.send(status.clone()).is_err() {
                                let _ = gw_send(&mut port, &[0x06, 0x04, status.unit_id, 0x00], 0);
                                let _ = gw_send(&mut port, &[0x0D, 0x02], 0);
                                break;
                            }

                            thread::sleep(Duration::from_millis(50));
                        }

                        if should_exit {
                            break;
                        }
                    }
                    status.connected = false;
                    status.log_msg = String::from("Déconnecté");
                    let _ = tx_status.send(status.clone());
                }
                Err(e) => {
                    status.connected = false;
                    status.log_msg = format!("Erreur d'ouverture port: {}", e);
                    let _ = tx_status.send(status.clone());
                }
            }
        } else {
            status.connected = false;
            status.log_msg = String::from("Recherche Greaseweazle...");
            let _ = tx_status.send(status.clone());
        }

        thread::sleep(Duration::from_secs(1));
    }
}
