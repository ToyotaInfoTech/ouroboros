// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

pub mod report;
use ouro_state::edge::EdgeSpec;

use crate::types::StateManager;

pub struct StateManagerImpl {
    desired_state: Option<EdgeSpec>,
    current_state: EdgeSpec,
}

impl StateManager for StateManagerImpl {
    fn set_desired_state(&mut self, state: EdgeSpec) {
        self.desired_state = Some(state);
        // Here you might want to implement logic to diff desired_state and current_state
        // and take necessary actions based on the differences.
    }

    fn get_current_state(&self) -> EdgeSpec {
        self.current_state.clone()
    }
}

#[cfg(test)]
mod tests {
    use ouro_schema::schema::shadow::report::SystemInfo;

    #[test]
    fn test_system_info_serialize_deserialize() {
        use sysinfo::System;
        let mut sys = System::new_all();
        sys.refresh_all();
        let info = SystemInfo::from(&sys);

        let bytes = {
            let mut buf = Vec::new();
            ciborium::ser::into_writer(&info, &mut buf).unwrap();
            buf
        };
        let decoded: SystemInfo = ciborium::from_reader(bytes.as_slice()).unwrap();
        assert_eq!(info, decoded);
    }
}
