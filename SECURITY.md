# Security Policy

## Supported Versions

The following versions of this project are currently supported with security updates:

| Version | Supported          |
| ------- | ------------------ |
| latest  | :white_check_mark: |

## Reporting a Vulnerability

We take security vulnerabilities seriously. If you discover a security issue, please follow the steps below:

1. **Do not** open a public GitHub issue for the vulnerability.
2. Send a detailed report to the project maintainers via email or through GitHub's private vulnerability reporting feature.
3. Include the following information in your report:
   - A description of the vulnerability
   - Steps to reproduce the issue
   - Potential impact of the vulnerability
   - Any suggested fixes or mitigations (if available)

## Response Process

- You will receive an acknowledgment of your report within **48 hours**.
- We will investigate the issue and provide updates on our progress.
- Once the vulnerability is confirmed, we will work on a fix and release a patch as soon as possible.
- We will notify you when the fix has been released.

## Disclosure Policy

We follow a **responsible disclosure** policy. We ask that you:

- Give us a reasonable amount of time to fix the issue before public disclosure.
- Avoid exploiting the vulnerability or accessing/modifying data that does not belong to you.
- Act in good faith to avoid privacy violations and disruption of services.

## Deployment Security

Ouroboros is a research prototype and is not hardened for untrusted networks.

- The WebUI and metrics servers listen on loopback by default. Set `OURO_WEBUI_TOKEN` before binding the WebUI to any other address.
- The metrics server and the sample Zenoh router configuration have no authentication. Restrict them with a firewall or security group.
- The WebUI token is sent in clear text over HTTP and DNS rebinding is not mitigated by the same-origin check alone. Use TLS or an SSH tunnel.

See [Security Considerations](docs/SECURITY_CONSIDERATIONS.md) for the details.

## Credits

We are grateful to security researchers and users who responsibly disclose vulnerabilities. Contributors may be acknowledged in release notes, unless they prefer to remain anonymous.
