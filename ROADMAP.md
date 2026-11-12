# Roadmap

This document tracks planned features and future directions for the Ouroboros platform.
Items listed here are not yet implemented and may change in priority or scope.

## Planned Features

### Data Sources — Sensor Simulation

- **Synthetic camera and LiDAR data streams**: Integrate a photo-realistic,
  GPU-accelerated simulator to generate synthetic sensor data per vehicle.
  This would allow end-to-end evaluation of data collection strategies beyond
  scalar telemetry (speed, position, lane).

### Daemon — Server Protocols

- **gRPC server**: Add a gRPC endpoint to the daemon's server layer as an
  alternative to the current HTTP/WebSocket interface. This would enable
  strongly-typed, bidirectional streaming RPC from external clients.

### Daemon — Messaging Clients

- **Kafka integration**: Add a Kafka client to the daemon's messaging layer,
  enabling high-throughput event streaming to external data pipelines and
  analytics platforms.

- **MQTT client**: Add an MQTT client to allow the daemon to interact with
  IoT-oriented brokers and devices that speak the MQTT protocol, broadening
  interoperability with standard connected-vehicle infrastructure.
