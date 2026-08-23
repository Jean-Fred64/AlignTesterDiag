//! Greaseweazle USB CDC binary protocol constants, opcodes, and packet constructors.
//!
//! Provides protocol-level definitions for communicating with Greaseweazle firmware
//! over USB CDC virtual COM ports at 115,200 baud.

#![allow(dead_code)]

use std::time::Duration;

// ============================================================================
// Protocol Command Opcodes
// ============================================================================

/// Query Greaseweazle device information / firmware version / sample frequency (0x00)
pub const CMD_GET_INFO: u8 = 0x00;
/// Firmware update mode (0x01)
pub const CMD_UPDATE: u8 = 0x01;
/// Step head carriage to specified cylinder (0x02)
pub const CMD_SEEK: u8 = 0x02;
/// Select physical head (Side 0 / Side 1) (0x03)
pub const CMD_HEAD: u8 = 0x03;
/// Set controller configuration parameters (0x04)
pub const CMD_SET_PARAMS: u8 = 0x04;
/// Get controller configuration parameters (0x05)
pub const CMD_GET_PARAMS: u8 = 0x05;
/// Control spindle motor power ON / OFF (0x06)
pub const CMD_MOTOR: u8 = 0x06;
/// Stream raw flux transition timings from drive (0x07)
pub const CMD_READ_FLUX: u8 = 0x07;
/// Write raw flux transitions to drive (0x08)
pub const CMD_WRITE_FLUX: u8 = 0x08;
/// Query flux status / error flags (0x09)
pub const CMD_GET_FLUX_STATUS: u8 = 0x09;
/// Retrieve index pulse timestamps (0x0A)
pub const CMD_GET_INDEX_TIMES: u8 = 0x0A;
/// Switch firmware operating mode (0x0B)
pub const CMD_SWITCH_FW_MODE: u8 = 0x0B;
/// Select physical drive unit (Drive A=0 / Drive B=1) (0x0C)
pub const CMD_SELECT: u8 = 0x0C;
/// Deselect all drive units / release interface (0x0D)
pub const CMD_DESELECT: u8 = 0x0D;
/// Configure bus type pinout (IBM PC / Shugart) (0x0E)
pub const CMD_SET_BUS_TYPE: u8 = 0x0E;
/// Set logic state of specified output pin (0x0F)
pub const CMD_SET_PIN: u8 = 0x0F;
/// Hardware reset / protocol re-sync opcode (0x00 info sync / 0x10 hard reset)
pub const CMD_RESET: u8 = 0x00;
/// Direct hardware reset (0x10)
pub const CMD_HARDWARE_RESET: u8 = 0x10;
/// Erase flux on current cylinder (0x11)
pub const CMD_ERASE_FLUX: u8 = 0x11;
/// Read logic level of physical connector pin (0x14)
pub const CMD_GET_PIN: u8 = 0x14;

// ============================================================================
// Command Acknowledgment Codes
// ============================================================================

/// Command executed successfully (0x00)
pub const ACK_OKAY: u8 = 0x00;
/// Unrecognized or malformed command opcode (0x01)
pub const ACK_BAD_COMMAND: u8 = 0x01;
/// No index pulse detected during operation (0x02)
pub const ACK_NO_INDEX: u8 = 0x02;
/// Track 0 optical stop signal not asserted during seek (0x03)
pub const ACK_NO_TRK0: u8 = 0x03;
/// USB / RAM flux buffer overflow (0x04)
pub const ACK_FLUX_OVERFLOW: u8 = 0x04;
/// Flux stream underflow (0x05)
pub const ACK_FLUX_UNDERFLOW: u8 = 0x05;
/// Diskette is write-protected (Pin 28 asserted) (0x06)
pub const ACK_WRPROT: u8 = 0x06;
/// Controller busy / no unit selected (0x07)
pub const ACK_NO_UNIT: u8 = 0x07;
/// Interface bus not enabled (0x08)
pub const ACK_NO_BUS: u8 = 0x08;
/// Invalid drive unit specified (0x09)
pub const ACK_BAD_UNIT: u8 = 0x09;
/// Invalid pin index (0x0A)
pub const ACK_BAD_PIN: u8 = 0x0A;
/// Invalid cylinder number (> 83) (0x0B)
pub const ACK_BAD_CYLINDER: u8 = 0x0B;

/// Greaseweazle floppy interface bus type pinout configuration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BusType {
    /// Standard IBM PC Floppy Bus (0x01)
    #[default]
    IbmPc = 0x01,
    /// Shugart Standard Floppy Bus (0x02, e.g. Amiga / Atari / Commodore / CPC native drives)
    Shugart = 0x02,
}

impl BusType {
    /// Returns the raw opcode payload byte for Greaseweazle CMD_SET_BUS_TYPE
    pub fn opcode_val(self) -> u8 {
        self as u8
    }

    /// Returns human-readable name of the bus interface
    pub fn as_str(self) -> &'static str {
        match self {
            BusType::IbmPc => "IBM PC",
            BusType::Shugart => "Shugart",
        }
    }

    /// Toggles between IBM PC and Shugart bus modes
    pub fn toggle(self) -> Self {
        match self {
            BusType::IbmPc => BusType::Shugart,
            BusType::Shugart => BusType::IbmPc,
        }
    }

    /// Converts a raw u8 value to BusType if valid
    pub fn from_u8(val: u8) -> Option<Self> {
        match val {
            0x01 => Some(BusType::IbmPc),
            0x02 => Some(BusType::Shugart),
            _ => None,
        }
    }
}

/// Floppy drive head carriage step multiplier mode:
/// - Single (1:1): Standard native stepping (96/135 TPI drives, up to physical track 83)
/// - Double (2:1): Double stepping for 48 TPI diskettes (40-41 logical tracks mapped to physical cylinders * 2)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StepMode {
    #[default]
    Single = 1,
    Double = 2,
}

impl StepMode {
    /// Toggles between Single (1:1) and Double (2:1) step modes
    pub fn toggle(&self) -> Self {
        match self {
            StepMode::Single => StepMode::Double,
            StepMode::Double => StepMode::Single,
        }
    }

    /// Physical track multiplier factor (1 for Single, 2 for Double)
    pub fn multiplier(&self) -> u8 {
        *self as u8
    }

    /// Maximum reachable logical track for the current mode (83 in Single, 41 in Double)
    pub fn max_logical_tracks(&self) -> u8 {
        match self {
            StepMode::Single => 83,
            StepMode::Double => 41,
        }
    }

    /// Human-readable label representation
    pub fn as_str(&self) -> &'static str {
        match self {
            StepMode::Single => "1:1 (Single)",
            StepMode::Double => "2:1 (Double)",
        }
    }
}

/// Multi-format retro disk profile
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DiskFormat {
    #[default]
    AutoDetect,
    IbmPc,
    AmigaDos,
    AtariSt,
    AmstradCpcData,
    AmstradCpcSystem,
}

impl DiskFormat {
    pub fn name(&self) -> &'static str {
        match self {
            DiskFormat::AutoDetect => "Auto-Detect",
            DiskFormat::IbmPc => "IBM PC (FAT)",
            DiskFormat::AmigaDos => "AmigaDOS (Paula)",
            DiskFormat::AtariSt => "Atari ST (WD1772)",
            DiskFormat::AmstradCpcData => "Amstrad CPC (DATA)",
            DiskFormat::AmstradCpcSystem => "Amstrad CPC (SYSTEM)",
        }
    }

    pub fn short_name(&self) -> &'static str {
        match self {
            DiskFormat::AutoDetect => "AUTO",
            DiskFormat::IbmPc => "PC",
            DiskFormat::AmigaDos => "AMIGA",
            DiskFormat::AtariSt => "ATARI",
            DiskFormat::AmstradCpcData => "CPC-DATA",
            DiskFormat::AmstradCpcSystem => "CPC-SYS",
        }
    }

    pub fn cycle_next(&self) -> Self {
        match self {
            DiskFormat::AutoDetect => DiskFormat::IbmPc,
            DiskFormat::IbmPc => DiskFormat::AmigaDos,
            DiskFormat::AmigaDos => DiskFormat::AtariSt,
            DiskFormat::AtariSt => DiskFormat::AmstradCpcData,
            DiskFormat::AmstradCpcData => DiskFormat::AmstradCpcSystem,
            DiskFormat::AmstradCpcSystem => DiskFormat::AutoDetect,
        }
    }

    pub fn expected_sector_count(&self, bitrate: u16, detected_count: u8) -> u8 {
        match self {
            DiskFormat::AmigaDos => {
                if bitrate == 500 {
                    22
                } else {
                    11
                }
            }
            DiskFormat::AmstradCpcData => {
                if detected_count >= 10 {
                    10
                } else {
                    9
                }
            }
            DiskFormat::AmstradCpcSystem => 9,
            DiskFormat::AtariSt => {
                if detected_count >= 11 {
                    11
                } else if detected_count == 10 {
                    10
                } else {
                    9
                }
            }
            DiskFormat::IbmPc => {
                if bitrate == 500 {
                    if detected_count == 15 {
                        15
                    } else {
                        18
                    }
                } else {
                    9
                }
            }
            DiskFormat::AutoDetect => {
                if bitrate == 500 {
                    if detected_count == 15 {
                        15
                    } else if detected_count == 22 {
                        22
                    } else if detected_count > 18 {
                        detected_count
                    } else {
                        18
                    }
                } else if detected_count == 11 {
                    11
                } else if detected_count == 10 {
                    10
                } else if detected_count > 0 {
                    detected_count
                } else {
                    9
                }
            }
        }
    }
}

/// Hardware Drive & Hybrid Format Presets
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PresetProfile {
    /// 3.5" HD (1.44M, PC Bus, Step 1:1, DPLL 500 kbps @ 300 RPM)
    #[default]
    Pc35Hd,
    /// 3.5" DD (720K, PC Bus, Step 1:1, DPLL 250 kbps @ 300 RPM)
    Pc35Dd,
    /// 5.25" HD (1.2M, PC Bus, Step 1:1, DPLL 500 kbps @ 360 RPM)
    Pc525Hd,
    /// 5.25" DD on HD Drive (360K, PC Bus, Step 2:1, DPLL 300 kbps @ 360 RPM)
    Pc525DdOnHd,
    /// 5.25" DD on DD Drive (360K, PC Bus, Step 1:1, DPLL 250 kbps @ 300 RPM)
    Pc525Dd,
    /// Amiga 3.5" (880K, Shugart Bus, Step 1:1, DPLL 250 kbps @ 300 RPM)
    Amiga35Dd,
    /// Atari 3.5" (720K, PC Bus, Step 1:1, DPLL 250 kbps @ 300 RPM)
    Atari35Dd,
    /// Amstrad CPC 3.0" (178K, Shugart Bus, Step 1:1, DPLL 250 kbps @ 300 RPM)
    Cpc30Data,
}

impl PresetProfile {
    /// Cycles to the next preset in standard sequential order
    pub fn next(&self) -> Self {
        match self {
            PresetProfile::Pc35Hd => PresetProfile::Pc35Dd,
            PresetProfile::Pc35Dd => PresetProfile::Pc525Hd,
            PresetProfile::Pc525Hd => PresetProfile::Pc525DdOnHd,
            PresetProfile::Pc525DdOnHd => PresetProfile::Pc525Dd,
            PresetProfile::Pc525Dd => PresetProfile::Amiga35Dd,
            PresetProfile::Amiga35Dd => PresetProfile::Atari35Dd,
            PresetProfile::Atari35Dd => PresetProfile::Cpc30Data,
            PresetProfile::Cpc30Data => PresetProfile::Pc35Hd,
        }
    }

    /// Alias for next()
    pub fn cycle_next(&self) -> Self {
        self.next()
    }

    /// Full human-readable descriptive label
    pub fn label(&self) -> &'static str {
        match self {
            PresetProfile::Pc35Hd => "3.5\" HD (1.44M)",
            PresetProfile::Pc35Dd => "3.5\" DD (720K)",
            PresetProfile::Pc525Hd => "5.25\" HD (1.2M)",
            PresetProfile::Pc525DdOnHd => "5.25\" DD on HD (360K)",
            PresetProfile::Pc525Dd => "5.25\" DD (360K)",
            PresetProfile::Amiga35Dd => "Amiga 3.5\" DD (880K)",
            PresetProfile::Atari35Dd => "Atari 3.5\" DD (720K)",
            PresetProfile::Cpc30Data => "Amstrad CPC 3.0\" (178K)",
        }
    }

    /// Short label for compact displays
    pub fn short_name(&self) -> &'static str {
        match self {
            PresetProfile::Pc35Hd => "3.5 HD",
            PresetProfile::Pc35Dd => "3.5 DD",
            PresetProfile::Pc525Hd => "5.25 HD",
            PresetProfile::Pc525DdOnHd => "5.25 DD@HD",
            PresetProfile::Pc525Dd => "5.25 DD",
            PresetProfile::Amiga35Dd => "Amiga 3.5",
            PresetProfile::Atari35Dd => "Atari 3.5",
            PresetProfile::Cpc30Data => "CPC 3.0",
        }
    }

    /// Target DPLL bit rate in kbps (500, 300, 250)
    pub fn target_data_rate(&self) -> u16 {
        match self {
            PresetProfile::Pc35Hd => 500,
            PresetProfile::Pc35Dd => 250,
            PresetProfile::Pc525Hd => 500,
            PresetProfile::Pc525DdOnHd => 300,
            PresetProfile::Pc525Dd => 250,
            PresetProfile::Amiga35Dd => 250,
            PresetProfile::Atari35Dd => 250,
            PresetProfile::Cpc30Data => 250,
        }
    }

    /// Nominal spindle speed in RPM
    pub fn target_rpm(&self) -> f64 {
        match self {
            PresetProfile::Pc525Hd | PresetProfile::Pc525DdOnHd => 360.0,
            _ => 300.0,
        }
    }

    /// Default floppy interface bus pinout
    pub fn default_bus(&self) -> BusType {
        match self {
            PresetProfile::Amiga35Dd | PresetProfile::Cpc30Data => BusType::Shugart,
            _ => BusType::IbmPc,
        }
    }

    /// Default head carriage step mode (Single 1:1 or Double 2:1)
    pub fn default_step(&self) -> StepMode {
        match self {
            PresetProfile::Pc525DdOnHd => StepMode::Double,
            _ => StepMode::Single,
        }
    }

    /// Default decoding disk format profile
    pub fn format_profile(&self) -> DiskFormat {
        match self {
            PresetProfile::Pc35Hd
            | PresetProfile::Pc35Dd
            | PresetProfile::Pc525Hd
            | PresetProfile::Pc525DdOnHd
            | PresetProfile::Pc525Dd => DiskFormat::IbmPc,
            PresetProfile::Amiga35Dd => DiskFormat::AmigaDos,
            PresetProfile::Atari35Dd => DiskFormat::AtariSt,
            PresetProfile::Cpc30Data => DiskFormat::AmstradCpcData,
        }
    }

    /// Case-insensitive parser supporting aliases for CLI and commands
    pub fn from_str_loose(s: &str) -> Option<Self> {
        let clean = s.trim().to_lowercase().replace(['-', '_', '.', ' ', '"', '\''], "");
        match clean.as_str() {
            "pc35hd" | "35hd" | "144m" | "144" | "hd35" | "pchd" => Some(PresetProfile::Pc35Hd),
            "pc35dd" | "35dd" | "720k" | "720" | "dd35" | "pcdd" => Some(PresetProfile::Pc35Dd),
            "pc525hd" | "525hd" | "12m" | "12" | "hd525" => Some(PresetProfile::Pc525Hd),
            "pc525ddonhd" | "525ddonhd" | "525ddhd" | "360khd" | "360onhd" | "360khigh" | "360k360rpm" => Some(PresetProfile::Pc525DdOnHd),
            "pc525dd" | "525dd" | "360k" | "360" | "dd525" => Some(PresetProfile::Pc525Dd),
            "amiga35dd" | "amiga35" | "amigadd" | "amiga" | "880k" | "880" | "amigados" => Some(PresetProfile::Amiga35Dd),
            "atari35dd" | "atari35" | "ataridd" | "atari" | "atarist" => Some(PresetProfile::Atari35Dd),
            "cpc30data" | "cpc30" | "cpcdata" | "cpc" | "178k" | "178" | "amstradcpc" => Some(PresetProfile::Cpc30Data),
            _ => None,
        }
    }
}

// ============================================================================
// Guard Timing & Timeout Constants
// ============================================================================

/// 500ms read guard timeout for ACK responses to prevent premature timeouts during physical pin toggles.
pub const ACK_GUARD_TIMEOUT_MS: u64 = 500;

/// Returns the standard ACK guard timeout duration.
pub fn ack_guard_timeout() -> Duration {
    Duration::from_millis(ACK_GUARD_TIMEOUT_MS)
}

// ============================================================================
// Packet Builders
// ============================================================================

/// Builds a 3-byte `CMD_RESET` / `CMD_GET_INFO` synchronization packet: `[0x00, 0x03, 0x00]`.
pub fn build_reset_packet() -> [u8; 3] {
    [CMD_RESET, 0x03, 0x00]
}

/// Builds a 4-byte `CMD_MOTOR` packet: `[0x06, 0x04, unit, state]`.
pub fn build_motor_packet(unit: u8, state: bool) -> [u8; 4] {
    [CMD_MOTOR, 0x04, unit, if state { 0x01 } else { 0x00 }]
}

/// Builds a 3-byte `CMD_HEAD` packet: `[0x03, 0x03, head]`.
pub fn build_head_packet(head: u8) -> [u8; 3] {
    [CMD_HEAD, 0x03, head]
}

/// Builds a 3-byte `CMD_SELECT` packet: `[0x0C, 0x03, unit]`.
pub fn build_select_packet(unit: u8) -> [u8; 3] {
    [CMD_SELECT, 0x03, unit]
}

/// Builds a 2-byte `CMD_DESELECT` packet: `[0x0D, 0x02]`.
pub fn build_deselect_packet() -> [u8; 2] {
    [CMD_DESELECT, 0x02]
}

/// Builds a 3-byte `CMD_SET_BUS_TYPE` packet: `[0x0E, 0x03, bus_type]`.
/// `0x01` = Standard IBM PC Floppy Bus.
/// `0x02` = Shugart Standard Floppy Bus.
pub fn build_bus_type_packet(bus_type: u8) -> [u8; 3] {
    [CMD_SET_BUS_TYPE, 0x03, bus_type]
}

/// Builds a 3-byte `CMD_SEEK` packet: `[0x02, 0x03, cyl]`.
pub fn build_seek_packet(cyl: u8) -> [u8; 3] {
    [CMD_SEEK, 0x03, cyl]
}

/// Builds an 8-byte `CMD_READ_FLUX` packet: `[0x07, 0x08, 0x00, 0x00, 0x00, 0x00, rev_low, rev_high]`.
pub fn build_read_flux_packet(revs: u16) -> [u8; 8] {
    let b_revs = revs.to_le_bytes();
    [CMD_READ_FLUX, 0x08, 0x00, 0x00, 0x00, 0x00, b_revs[0], b_revs[1]]
}

/// Builds a 3-byte `CMD_GET_PIN` packet: `[0x14, 0x03, pin_num]`.
pub fn build_get_pin_packet(pin_num: u8) -> [u8; 3] {
    [CMD_GET_PIN, 0x03, pin_num]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_opcodes_values() {
        assert_eq!(CMD_GET_INFO, 0x00);
        assert_eq!(CMD_RESET, 0x00);
        assert_eq!(CMD_SEEK, 0x02);
        assert_eq!(CMD_HEAD, 0x03);
        assert_eq!(CMD_MOTOR, 0x06);
        assert_eq!(CMD_READ_FLUX, 0x07);
        assert_eq!(CMD_SELECT, 0x0C);
        assert_eq!(CMD_DESELECT, 0x0D);
        assert_eq!(CMD_SET_BUS_TYPE, 0x0E);
        assert_eq!(CMD_GET_PIN, 0x14);
        assert_eq!(ACK_GUARD_TIMEOUT_MS, 500);
    }

    #[test]
    fn test_bus_type_enum() {
        assert_eq!(BusType::default(), BusType::IbmPc);
        assert_eq!(BusType::IbmPc.opcode_val(), 0x01);
        assert_eq!(BusType::Shugart.opcode_val(), 0x02);
        assert_eq!(BusType::IbmPc.as_str(), "IBM PC");
        assert_eq!(BusType::Shugart.as_str(), "Shugart");
        assert_eq!(BusType::IbmPc.toggle(), BusType::Shugart);
        assert_eq!(BusType::Shugart.toggle(), BusType::IbmPc);
        assert_eq!(BusType::from_u8(1), Some(BusType::IbmPc));
        assert_eq!(BusType::from_u8(2), Some(BusType::Shugart));
        assert_eq!(BusType::from_u8(0), None);
        assert_eq!(BusType::from_u8(3), None);
    }

    #[test]
    fn test_step_mode_enum() {
        assert_eq!(StepMode::default(), StepMode::Single);
        assert_eq!(StepMode::Single.multiplier(), 1);
        assert_eq!(StepMode::Double.multiplier(), 2);
        assert_eq!(StepMode::Single.max_logical_tracks(), 83);
        assert_eq!(StepMode::Double.max_logical_tracks(), 41);
        assert_eq!(StepMode::Single.as_str(), "1:1 (Single)");
        assert_eq!(StepMode::Double.as_str(), "2:1 (Double)");
        assert_eq!(StepMode::Single.toggle(), StepMode::Double);
        assert_eq!(StepMode::Double.toggle(), StepMode::Single);
    }

    #[test]
    fn test_protocol_packet_builders() {
        assert_eq!(build_reset_packet(), [0x00, 0x03, 0x00]);
        assert_eq!(build_motor_packet(0, true), [0x06, 0x04, 0x00, 0x01]);
        assert_eq!(build_motor_packet(1, false), [0x06, 0x04, 0x01, 0x00]);
        assert_eq!(build_head_packet(1), [0x03, 0x03, 0x01]);
        assert_eq!(build_select_packet(0), [0x0C, 0x03, 0x00]);
        assert_eq!(build_deselect_packet(), [0x0D, 0x02]);
        assert_eq!(build_bus_type_packet(BusType::IbmPc.opcode_val()), [0x0E, 0x03, 0x01]);
        assert_eq!(build_bus_type_packet(BusType::Shugart.opcode_val()), [0x0E, 0x03, 0x02]);
        assert_eq!(build_seek_packet(40), [0x02, 0x03, 40]);
        assert_eq!(build_read_flux_packet(3), [0x07, 0x08, 0x00, 0x00, 0x00, 0x00, 0x03, 0x00]);
        assert_eq!(build_get_pin_packet(28), [0x14, 0x03, 28]);
    }

    #[test]
    fn test_preset_profile_cycle_sequence() {
        let mut p = PresetProfile::Pc35Hd;
        assert_eq!(p, PresetProfile::default());

        p = p.next();
        assert_eq!(p, PresetProfile::Pc35Dd);
        assert_eq!(p.target_data_rate(), 250);
        assert_eq!(p.default_bus(), BusType::IbmPc);
        assert_eq!(p.default_step(), StepMode::Single);
        assert_eq!(p.format_profile(), DiskFormat::IbmPc);
        assert_eq!(p.target_rpm(), 300.0);

        p = p.next();
        assert_eq!(p, PresetProfile::Pc525Hd);
        assert_eq!(p.target_data_rate(), 500);
        assert_eq!(p.default_bus(), BusType::IbmPc);
        assert_eq!(p.default_step(), StepMode::Single);
        assert_eq!(p.format_profile(), DiskFormat::IbmPc);
        assert_eq!(p.target_rpm(), 360.0);

        p = p.next();
        assert_eq!(p, PresetProfile::Pc525DdOnHd);
        assert_eq!(p.target_data_rate(), 300);
        assert_eq!(p.default_bus(), BusType::IbmPc);
        assert_eq!(p.default_step(), StepMode::Double);
        assert_eq!(p.format_profile(), DiskFormat::IbmPc);
        assert_eq!(p.target_rpm(), 360.0);

        p = p.next();
        assert_eq!(p, PresetProfile::Pc525Dd);
        assert_eq!(p.target_data_rate(), 250);
        assert_eq!(p.default_bus(), BusType::IbmPc);
        assert_eq!(p.default_step(), StepMode::Single);
        assert_eq!(p.format_profile(), DiskFormat::IbmPc);
        assert_eq!(p.target_rpm(), 300.0);

        p = p.next();
        assert_eq!(p, PresetProfile::Amiga35Dd);
        assert_eq!(p.target_data_rate(), 250);
        assert_eq!(p.default_bus(), BusType::Shugart);
        assert_eq!(p.default_step(), StepMode::Single);
        assert_eq!(p.format_profile(), DiskFormat::AmigaDos);
        assert_eq!(p.target_rpm(), 300.0);

        p = p.next();
        assert_eq!(p, PresetProfile::Atari35Dd);
        assert_eq!(p.target_data_rate(), 250);
        assert_eq!(p.default_bus(), BusType::IbmPc);
        assert_eq!(p.default_step(), StepMode::Single);
        assert_eq!(p.format_profile(), DiskFormat::AtariSt);
        assert_eq!(p.target_rpm(), 300.0);

        p = p.next();
        assert_eq!(p, PresetProfile::Cpc30Data);
        assert_eq!(p.target_data_rate(), 250);
        assert_eq!(p.default_bus(), BusType::Shugart);
        assert_eq!(p.default_step(), StepMode::Single);
        assert_eq!(p.format_profile(), DiskFormat::AmstradCpcData);
        assert_eq!(p.target_rpm(), 300.0);

        p = p.next();
        assert_eq!(p, PresetProfile::Pc35Hd);
        assert_eq!(p.target_data_rate(), 500);
        assert_eq!(p.default_bus(), BusType::IbmPc);
        assert_eq!(p.default_step(), StepMode::Single);
        assert_eq!(p.format_profile(), DiskFormat::IbmPc);
        assert_eq!(p.target_rpm(), 300.0);
    }

    #[test]
    fn test_preset_profile_from_str_loose() {
        assert_eq!(PresetProfile::from_str_loose("pc35hd"), Some(PresetProfile::Pc35Hd));
        assert_eq!(PresetProfile::from_str_loose("1.44M"), Some(PresetProfile::Pc35Hd));
        assert_eq!(PresetProfile::from_str_loose("pc35dd"), Some(PresetProfile::Pc35Dd));
        assert_eq!(PresetProfile::from_str_loose("720k"), Some(PresetProfile::Pc35Dd));
        assert_eq!(PresetProfile::from_str_loose("pc525hd"), Some(PresetProfile::Pc525Hd));
        assert_eq!(PresetProfile::from_str_loose("1.2M"), Some(PresetProfile::Pc525Hd));
        assert_eq!(PresetProfile::from_str_loose("pc525ddonhd"), Some(PresetProfile::Pc525DdOnHd));
        assert_eq!(PresetProfile::from_str_loose("360k-hd"), Some(PresetProfile::Pc525DdOnHd));
        assert_eq!(PresetProfile::from_str_loose("pc525dd"), Some(PresetProfile::Pc525Dd));
        assert_eq!(PresetProfile::from_str_loose("360K"), Some(PresetProfile::Pc525Dd));
        assert_eq!(PresetProfile::from_str_loose("amiga"), Some(PresetProfile::Amiga35Dd));
        assert_eq!(PresetProfile::from_str_loose("880k"), Some(PresetProfile::Amiga35Dd));
        assert_eq!(PresetProfile::from_str_loose("atari"), Some(PresetProfile::Atari35Dd));
        assert_eq!(PresetProfile::from_str_loose("cpc"), Some(PresetProfile::Cpc30Data));
        assert_eq!(PresetProfile::from_str_loose("178k"), Some(PresetProfile::Cpc30Data));
        assert_eq!(PresetProfile::from_str_loose("invalid_preset"), None);
    }
}
