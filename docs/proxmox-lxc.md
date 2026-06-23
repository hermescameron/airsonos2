# Proxmox LXC Notes

AirSonos2 works best when the container behaves like it is on the same LAN as iPhones and Sonos speakers.

Recommended setup:

- Use bridged or host-like networking for the LXC.
- Ensure multicast and mDNS pass between client VLANs and the Sonos VLAN.
- Allow Sonos TCP port `1400` from the LXC to each speaker.
- Allow the AirSonos2 HTTP stream port, default `7000`, from each Sonos speaker to the LXC.
- Allow one RTSP TCP port per virtual AirPlay endpoint, starting at `airplay.base_rtsp_port`.
- Keep `ffmpeg` installed in the runtime environment.

Run:

```bash
airsonos2 doctor --config /etc/airsonos2/config.toml
```

`doctor` can check local ports, SSDP discovery, Sonos SOAP reachability, and `ffmpeg`. Full mDNS visibility and AirPlay timing checks still require a second host or iOS device on the target network.
