use std::borrow::Cow;

use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SoapService {
    AvTransport,
    RenderingControl,
    ZoneGroupTopology,
}

impl SoapService {
    pub fn control_path(self) -> &'static str {
        match self {
            Self::AvTransport => "/MediaRenderer/AVTransport/Control",
            Self::RenderingControl => "/MediaRenderer/RenderingControl/Control",
            Self::ZoneGroupTopology => "/ZoneGroupTopology/Control",
        }
    }

    pub fn urn(self) -> &'static str {
        match self {
            Self::AvTransport => "urn:schemas-upnp-org:service:AVTransport:1",
            Self::RenderingControl => "urn:schemas-upnp-org:service:RenderingControl:1",
            Self::ZoneGroupTopology => "urn:schemas-upnp-org:service:ZoneGroupTopology:1",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SoapAction {
    pub service: SoapService,
    pub action: &'static str,
}

impl SoapAction {
    pub const SET_AV_TRANSPORT_URI: Self = Self {
        service: SoapService::AvTransport,
        action: "SetAVTransportURI",
    };
    pub const PLAY: Self = Self {
        service: SoapService::AvTransport,
        action: "Play",
    };
    pub const PAUSE: Self = Self {
        service: SoapService::AvTransport,
        action: "Pause",
    };
    pub const STOP: Self = Self {
        service: SoapService::AvTransport,
        action: "Stop",
    };
    pub const BECOME_STANDALONE: Self = Self {
        service: SoapService::AvTransport,
        action: "BecomeCoordinatorOfStandaloneGroup",
    };
    pub const SET_VOLUME: Self = Self {
        service: SoapService::RenderingControl,
        action: "SetVolume",
    };
    pub const GET_VOLUME: Self = Self {
        service: SoapService::RenderingControl,
        action: "GetVolume",
    };
    pub const GET_ZONE_GROUP_STATE: Self = Self {
        service: SoapService::ZoneGroupTopology,
        action: "GetZoneGroupState",
    };

    pub fn soap_action_header(self) -> String {
        format!("\"{}#{}\"", self.service.urn(), self.action)
    }
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum SoapBuildError {
    #[error("volume must be in 0..=100, got {0}")]
    VolumeOutOfRange(u8),
}

pub fn envelope(action: SoapAction, inner_xml: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <u:{action} xmlns:u="{urn}">
{inner_xml}
    </u:{action}>
  </s:Body>
</s:Envelope>"#,
        action = action.action,
        urn = action.service.urn(),
    )
}

pub fn set_av_transport_uri_body(uri: &str, metadata: &str) -> String {
    envelope(
        SoapAction::SET_AV_TRANSPORT_URI,
        &format!(
            "      <InstanceID>0</InstanceID>\n      <CurrentURI>{}</CurrentURI>\n      <CurrentURIMetaData>{}</CurrentURIMetaData>",
            escape_xml(uri),
            escape_xml(metadata),
        ),
    )
}

pub fn play_body() -> String {
    envelope(
        SoapAction::PLAY,
        "      <InstanceID>0</InstanceID>\n      <Speed>1</Speed>",
    )
}

pub fn pause_body() -> String {
    envelope(SoapAction::PAUSE, "      <InstanceID>0</InstanceID>")
}

pub fn stop_body() -> String {
    envelope(SoapAction::STOP, "      <InstanceID>0</InstanceID>")
}

pub fn become_standalone_body() -> String {
    envelope(
        SoapAction::BECOME_STANDALONE,
        "      <InstanceID>0</InstanceID>",
    )
}

pub fn set_volume_body(volume: u8) -> Result<String, SoapBuildError> {
    if volume > 100 {
        return Err(SoapBuildError::VolumeOutOfRange(volume));
    }

    Ok(envelope(
        SoapAction::SET_VOLUME,
        &format!(
            "      <InstanceID>0</InstanceID>\n      <Channel>Master</Channel>\n      <DesiredVolume>{volume}</DesiredVolume>",
        ),
    ))
}

pub fn get_volume_body() -> String {
    envelope(
        SoapAction::GET_VOLUME,
        "      <InstanceID>0</InstanceID>\n      <Channel>Master</Channel>",
    )
}

pub fn parse_get_volume_response(xml: &str) -> Option<u8> {
    let start = xml.find("<CurrentVolume>")?;
    let value_start = start + "<CurrentVolume>".len();
    let value_end = xml[value_start..].find("</CurrentVolume>")?;
    let value = xml[value_start..value_start + value_end].trim();
    value.parse().ok().filter(|&volume| volume <= 100)
}

pub fn get_zone_group_state_body() -> String {
    envelope(SoapAction::GET_ZONE_GROUP_STATE, "")
}

pub fn set_av_transport_uri_metadata(title: &str) -> String {
    format!(
        r#"<DIDL-Lite xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/" xmlns:r="urn:schemas-rinconnetworks-com:metadata-1-0/" xmlns="urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/"><item id="airsonos2" parentID="0" restricted="true"><dc:title>{}</dc:title><upnp:class>object.item.audioItem.audioBroadcast</upnp:class></item></DIDL-Lite>"#,
        escape_xml(title),
    )
}

fn escape_xml(value: &str) -> Cow<'_, str> {
    if !value.contains(['&', '<', '>', '"', '\'']) {
        return Cow::Borrowed(value);
    }

    Cow::Owned(
        value
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&apos;"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_set_transport_uri_request() {
        let body = set_av_transport_uri_body(
            "http://192.0.2.10:7000/streams/a.mp3",
            &set_av_transport_uri_metadata("Kitchen & Office"),
        );

        assert!(body.contains("<u:SetAVTransportURI"));
        assert!(body.contains("http://192.0.2.10:7000/streams/a.mp3"));
        assert!(body.contains("Kitchen &amp;amp; Office"));
    }

    #[test]
    fn set_volume_rejects_out_of_range_values() {
        assert!(set_volume_body(100).is_ok());
        assert_eq!(
            set_volume_body(101),
            Err(SoapBuildError::VolumeOutOfRange(101))
        );
    }

    #[test]
    fn parses_get_volume_response() {
        let xml = r#"<?xml version="1.0" ?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
<s:Body>
<u:GetVolumeResponse xmlns:u="urn:schemas-upnp-org:service:RenderingControl:1">
<CurrentVolume>42</CurrentVolume>
</u:GetVolumeResponse>
</s:Body>
</s:Envelope>"#;

        assert_eq!(parse_get_volume_response(xml), Some(42));
    }

    #[test]
    fn soap_action_header_matches_sonos_format() {
        assert_eq!(
            SoapAction::PLAY.soap_action_header(),
            "\"urn:schemas-upnp-org:service:AVTransport:1#Play\""
        );
    }

    #[test]
    fn generates_pause_request() {
        let body = pause_body();

        assert!(body.contains("<u:Pause"));
        assert!(body.contains("<InstanceID>0</InstanceID>"));
        assert_eq!(
            SoapAction::PAUSE.soap_action_header(),
            "\"urn:schemas-upnp-org:service:AVTransport:1#Pause\""
        );
    }
}
