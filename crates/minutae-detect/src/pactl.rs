//! Parse `pactl list source-outputs` and `pactl list short sources` (Linux).
//! Pure functions so they're testable on every OS.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceOutput {
    pub index: u32,
    pub source: Option<u32>,
    pub corked: bool,
    pub app_name: Option<String>,
    pub binary: Option<String>,
    pub app_id: Option<String>,
    pub pid: Option<u32>,
    pub media_name: Option<String>,
}

/// Parse the long-form `pactl list source-outputs` output. Property values
/// are quoted (`application.name = "Firefox"`). Works with PulseAudio and
/// pipewire-pulse. Assumes `LC_ALL=C` (English headers).
pub fn parse_source_outputs(text: &str) -> Vec<SourceOutput> {
    let mut out = Vec::new();
    let mut cur: Option<SourceOutput> = None;
    for line in text.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("Source Output #") {
            if let Some(c) = cur.take() {
                out.push(c);
            }
            cur = Some(SourceOutput {
                index: rest.trim().parse().unwrap_or(0),
                source: None,
                corked: false,
                app_name: None,
                binary: None,
                app_id: None,
                pid: None,
                media_name: None,
            });
            continue;
        }
        let Some(c) = cur.as_mut() else { continue };
        if let Some(v) = t.strip_prefix("Source:") {
            c.source = v.trim().parse().ok();
        } else if let Some(v) = t.strip_prefix("Corked:") {
            c.corked = v.trim() == "yes";
        } else if let Some((k, v)) = t.split_once(" = ") {
            let v = v.trim().trim_matches('"').to_string();
            match k.trim() {
                "application.name" => c.app_name = Some(v),
                "application.process.binary" => c.binary = Some(v),
                "application.id" => c.app_id = Some(v),
                "application.process.id" => c.pid = v.parse().ok(),
                "media.name" => c.media_name = Some(v),
                _ => {}
            }
        }
    }
    if let Some(c) = cur {
        out.push(c);
    }
    out
}

/// Parse `pactl list short sources` into (index, name).
pub fn parse_short_sources(text: &str) -> Vec<(u32, String)> {
    text.lines()
        .filter_map(|l| {
            let mut it = l.split('\t');
            let idx = it.next()?.trim().parse().ok()?;
            let name = it.next()?.trim().to_string();
            Some((idx, name))
        })
        .collect()
}

/// Apps that open capture streams without being a call (level meters, us).
const IGNORED_APPS: &[&str] = &[
    "org.pulseaudio.pavucontrol",
    "pavucontrol",
    "org.gnome.settings",
    "gnome-control-center",
    "kmix",
    "plasma-pa",
    "org.kde.plasma",
    "easyeffects",
    "com.github.wwmm.easyeffects",
    "minutae",
];

/// Select the apps actually recording a microphone: not corked, not reading
/// a `.monitor` source, not a known meter, not our own pid.
pub fn active_mic_apps(
    outputs: &[SourceOutput],
    sources: &[(u32, String)],
    own_pid: u32,
) -> Vec<String> {
    let mut apps = Vec::new();
    for o in outputs {
        if o.corked || o.pid == Some(own_pid) {
            continue;
        }
        let src_name = o
            .source
            .and_then(|s| sources.iter().find(|(i, _)| *i == s))
            .map(|(_, n)| n);
        if src_name.is_some_and(|n| n.ends_with(".monitor")) {
            continue;
        }
        let ids = [&o.app_id, &o.app_name, &o.binary];
        let ignored = ids.iter().any(|v| {
            v.as_deref().is_some_and(|v| {
                let l = v.to_ascii_lowercase();
                IGNORED_APPS
                    .iter()
                    .any(|i| l == *i || l.starts_with(&format!("{i}.")))
            })
        });
        if ignored {
            continue;
        }
        let name = o
            .app_name
            .as_deref()
            .or(o.binary.as_deref())
            .map(crate::names::from_exe)
            .unwrap_or_else(|| "Unknown app".to_string());
        if !apps.contains(&name) {
            apps.push(name);
        }
    }
    apps
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUTPUTS: &str = r#"Source Output #42
	Driver: PipeWire
	Owner Module: n/a
	Client: 71
	Source: 55
	Sample Specification: float32le 1ch 48000Hz
	Channel Map: mono
	Format: pcm, format.sample_format = "\"float32le\""  format.rate = "48000"  format.channels = "1"  format.channel_map = "\"mono\""
	Corked: no
	Mute: no
	Volume: mono: 65536 / 100% / 0.00 dB
	        balance 0.00
	Buffer Latency: 0 usec
	Source Latency: 0 usec
	Resample method: PipeWire
	Properties:
		client.api = "pipewire-pulse"
		application.name = "ZOOM VoiceEngine"
		application.process.id = "4242"
		application.process.binary = "zoom"
		media.name = "record"

Source Output #43
	Driver: PipeWire
	Source: 56
	Corked: no
	Properties:
		application.name = "Firefox"
		application.process.binary = "firefox"
		application.process.id = "777"

Source Output #44
	Driver: PipeWire
	Source: 55
	Corked: no
	Properties:
		application.name = "PulseAudio Volume Control"
		application.id = "org.PulseAudio.pavucontrol"
		application.process.id = "900"

Source Output #45
	Source: 55
	Corked: yes
	Properties:
		application.name = "Slack"
		application.process.id = "901"

Source Output #46
	Source: 55
	Corked: no
	Properties:
		application.name = "minutae"
		application.process.id = "1234"
"#;

    const SOURCES: &str = "55\talsa_input.pci-0000_00_1f.3.analog-stereo\tPipeWire\ts32le 2ch 48000Hz\tRUNNING\n56\talsa_output.pci-0000_00_1f.3.analog-stereo.monitor\tPipeWire\ts32le 2ch 48000Hz\tIDLE\n";

    #[test]
    fn parses_blocks() {
        let o = parse_source_outputs(OUTPUTS);
        assert_eq!(o.len(), 5);
        assert_eq!(o[0].index, 42);
        assert_eq!(o[0].source, Some(55));
        assert_eq!(o[0].app_name.as_deref(), Some("ZOOM VoiceEngine"));
        assert_eq!(o[0].pid, Some(4242));
        assert_eq!(o[0].binary.as_deref(), Some("zoom"));
        assert!(o[3].corked);
        assert_eq!(o[2].app_id.as_deref(), Some("org.PulseAudio.pavucontrol"));
    }

    #[test]
    fn parses_short_sources() {
        let s = parse_short_sources(SOURCES);
        assert_eq!(s.len(), 2);
        assert!(s[1].1.ends_with(".monitor"));
    }

    #[test]
    fn filters_to_real_mic_users() {
        let apps = active_mic_apps(
            &parse_source_outputs(OUTPUTS),
            &parse_short_sources(SOURCES),
            1234,
        );
        // Firefox reads a monitor, pavucontrol is a meter, Slack is corked, 1234 is us.
        assert_eq!(apps, vec!["Zoom".to_string()]);
    }

    #[test]
    fn empty_output() {
        assert!(parse_source_outputs("").is_empty());
        assert!(active_mic_apps(&[], &[], 1).is_empty());
    }
}
