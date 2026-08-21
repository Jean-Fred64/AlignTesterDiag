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
}
