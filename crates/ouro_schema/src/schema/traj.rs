// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Trajectory and telemetry event schema definitions.
//!
//! This module defines data structures for vehicle telemetry events,
//! location tracking, and trajectory recording.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// Vehicle identification information.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Vehicle {
    /// Vehicle Identification Number.
    pub vin: String,
    /// Vehicle model identifier.
    pub model: String,
}

impl Vehicle {
    /// Creates a new Vehicle with the given VIN and model.
    ///
    /// # Arguments
    /// * `vin` - Vehicle Identification Number
    /// * `model` - Vehicle model identifier
    ///
    /// # Returns
    /// A new `Vehicle` instance
    pub fn new<S: Into<String>>(vin: S, model: S) -> Self {
        Self {
            vin: vin.into(),
            model: model.into(),
        }
    }
}

/// A single GPS location fix with timestamp.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, sqlx::FromRow)]
pub struct LocationFix {
    /// Timestamp of the location reading.
    pub timestamp: time::OffsetDateTime,
    /// Latitude in decimal degrees.
    pub latitude: f64,
    /// Longitude in decimal degrees.
    pub longitude: f64,
}

impl LocationFix {
    /// Creates a new LocationFix.
    ///
    /// # Arguments
    /// * `timestamp` - When the fix was recorded
    /// * `latitude` - Latitude in decimal degrees
    /// * `longitude` - Longitude in decimal degrees
    ///
    /// # Returns
    /// A new `LocationFix` instance
    pub fn new(timestamp: time::OffsetDateTime, latitude: f64, longitude: f64) -> Self {
        Self {
            timestamp,
            latitude,
            longitude,
        }
    }
}

/// A sequence of location fixes representing a vehicle's path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Trajectory {
    /// Ordered list of location fixes.
    pub location_fixes: Vec<LocationFix>,
}

impl Trajectory {
    /// Creates a new empty trajectory.
    ///
    /// # Returns
    /// A new `Trajectory` with no location fixes
    pub fn new() -> Self {
        Self {
            location_fixes: Vec::new(),
        }
    }

    /// Appends a location fix to the trajectory.
    ///
    /// # Arguments
    /// * `fix` - Location fix to append
    pub fn add_location_fix(&mut self, fix: LocationFix) {
        self.location_fixes.push(fix);
    }
}

impl Default for Trajectory {
    fn default() -> Self {
        Self::new()
    }
}

/// A telemetry event containing metadata.
pub struct Event {
    /// Metadata describing the event.
    pub metadata: EventMetadata,
}

impl Event {
    /// Returns the vehicle associated with this event.
    pub fn vehicle(&self) -> &Vehicle {
        &self.metadata.vehicle
    }

    /// Returns the event identifier.
    pub fn event_id(&self) -> &str {
        &self.metadata.event_id
    }

    /// Returns the start timestamp of the event.
    pub fn timestamp(&self) -> time::OffsetDateTime {
        self.metadata.start_time
    }
}

/// Camera identifier for the 6-camera surround-view system.
///
/// Cameras are spaced at 60-degree yaw intervals with 120-degree HFOV
/// to provide full 360-degree coverage with no blind spots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraId {
    /// Forward-facing camera (0° yaw).
    Front,
    /// Front-left camera (60° yaw).
    FrontLeft,
    /// Front-right camera (−60° yaw).
    FrontRight,
    /// Rear-left camera (120° yaw).
    RearLeft,
    /// Rear-right camera (−120° yaw).
    RearRight,
    /// Rear-facing camera (180° yaw).
    Rear,
}

impl fmt::Display for CameraId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            CameraId::Front => "front",
            CameraId::FrontLeft => "front_left",
            CameraId::FrontRight => "front_right",
            CameraId::RearLeft => "rear_left",
            CameraId::RearRight => "rear_right",
            CameraId::Rear => "rear",
        };
        f.write_str(s)
    }
}

impl FromStr for CameraId {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "front" => Ok(CameraId::Front),
            "front_left" => Ok(CameraId::FrontLeft),
            "front_right" => Ok(CameraId::FrontRight),
            "rear_left" => Ok(CameraId::RearLeft),
            "rear_right" => Ok(CameraId::RearRight),
            "rear" => Ok(CameraId::Rear),
            _ => Err(format!("unknown camera id: {}", s)),
        }
    }
}

/// A single camera frame captured at a specific timestamp.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CameraFrame {
    /// Camera identifier.
    pub camera_id: CameraId,
    /// Timestamp of the frame capture.
    pub timestamp: time::OffsetDateTime,
    /// JPEG-compressed image bytes.
    pub image: Vec<u8>,
}

/// A behavior activation span observed within a metadata window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActiveBehavior {
    /// Behavior name (matches `@behavior(name=...)` in Python).
    pub name: String,
    /// Timestamp when the behavior was first observed active.
    pub start_time: time::OffsetDateTime,
    /// Timestamp when the behavior was last observed active.
    pub end_time: time::OffsetDateTime,
}

/// Metadata describing a telemetry event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventMetadata {
    /// Vehicle that generated this event.
    pub vehicle: Vehicle,
    /// Unique event identifier.
    pub event_id: String,
    /// Event start timestamp.
    pub start_time: time::OffsetDateTime,
    /// Event end timestamp.
    pub end_time: time::OffsetDateTime,
    /// Trajectory data recorded during this event.
    pub trajectory: Trajectory,
    /// Additional key-value attributes.
    pub attributes: std::collections::BTreeMap<String, String>,
    /// Camera frames captured during this event (1Hz subsampled).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub camera_frames: Option<Vec<CameraFrame>>,
    /// Behavior activation spans observed during this event.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub active_behaviors: Option<Vec<ActiveBehavior>>,
}

impl EventMetadata {
    /// Creates a new EventMetadata with default timestamps.
    ///
    /// # Arguments
    /// * `vehicle` - Vehicle associated with the event
    /// * `event_id` - Unique event identifier
    ///
    /// # Returns
    /// A new `EventMetadata` with UNIX_EPOCH timestamps and empty trajectory
    pub fn new(vehicle: &Vehicle, event_id: &str) -> Self {
        Self {
            vehicle: vehicle.clone(),
            event_id: event_id.to_string(),
            start_time: OffsetDateTime::UNIX_EPOCH,
            end_time: OffsetDateTime::UNIX_EPOCH,
            trajectory: Trajectory::new(),
            attributes: std::collections::BTreeMap::new(),
            camera_frames: None,
            active_behaviors: None,
        }
    }

    /// Returns a mutable reference to the trajectory.
    pub fn trajectory(&mut self) -> &mut Trajectory {
        &mut self.trajectory
    }

    /// Returns a mutable reference to the attributes map.
    pub fn attributes(&mut self) -> &mut std::collections::BTreeMap<String, String> {
        &mut self.attributes
    }
}

/// Record tracking a sent CBOR-encoded message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, sqlx::FromRow)]
pub struct SendLog {
    /// Unique log entry identifier.
    pub id: i64,
    /// CBOR-encoded message payload.
    pub cbor: Vec<u8>,
}

impl SendLog {
    /// Creates a new SendLog entry.
    ///
    /// # Arguments
    /// * `id` - Unique log entry identifier
    /// * `cbor` - CBOR-encoded message bytes
    ///
    /// # Returns
    /// A new `SendLog` instance
    pub fn new(id: i64, cbor: &[u8]) -> Self {
        Self {
            id,
            cbor: cbor.to_vec(),
        }
    }
}
