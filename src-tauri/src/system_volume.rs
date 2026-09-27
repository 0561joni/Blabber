use anyhow::{anyhow, Context, Result};

const DUCK_FACTOR: f32 = 0.30;

#[derive(Debug, Clone)]
pub struct VolumeSnapshot {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    volume: f32,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    muted: bool,
}

pub fn duck_to_30_percent_of_current() -> Result<Option<VolumeSnapshot>> {
    #[cfg(target_os = "macos")]
    {
        return macos::duck_to_30_percent_of_current();
    }

    #[cfg(target_os = "windows")]
    {
        return windows_impl::duck_to_30_percent_of_current();
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Ok(None)
    }
}

pub fn restore(snapshot: VolumeSnapshot) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        return macos::restore(snapshot);
    }

    #[cfg(target_os = "windows")]
    {
        return windows_impl::restore(snapshot);
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = snapshot;
        Ok(())
    }
}

fn ducked_volume(current: f32) -> f32 {
    (current.clamp(0.0, 1.0) * DUCK_FACTOR).clamp(0.0, 1.0)
}

#[cfg(target_os = "macos")]
mod macos {
    use std::process::Command;

    use super::*;

    pub fn duck_to_30_percent_of_current() -> Result<Option<VolumeSnapshot>> {
        let snapshot = read_volume_settings()?;
        if snapshot.muted || snapshot.volume <= 0.0 {
            return Ok(None);
        }

        let ducked_percent = (ducked_volume(snapshot.volume) * 100.0).round() as u8;
        run_osascript(&format!("set volume output volume {ducked_percent}"))?;
        Ok(Some(snapshot))
    }

    pub fn restore(snapshot: VolumeSnapshot) -> Result<()> {
        let volume_percent = (snapshot.volume.clamp(0.0, 1.0) * 100.0).round() as u8;
        run_osascript(&format!("set volume output volume {volume_percent}"))?;
        if snapshot.muted {
            run_osascript("set volume with output muted")?;
        } else {
            run_osascript("set volume without output muted")?;
        }
        Ok(())
    }

    fn read_volume_settings() -> Result<VolumeSnapshot> {
        let output = Command::new("osascript")
            .args(["-e", "get volume settings"])
            .output()
            .context("failed to query macOS output volume")?;

        if !output.status.success() {
            return Err(anyhow!(
                "macOS output volume query failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }

        parse_volume_settings(&String::from_utf8_lossy(&output.stdout))
    }

    fn run_osascript(script: &str) -> Result<()> {
        let output = Command::new("osascript")
            .args(["-e", script])
            .output()
            .with_context(|| format!("failed to run osascript: {script}"))?;

        if output.status.success() {
            Ok(())
        } else {
            Err(anyhow!(
                "osascript failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
    }

    fn parse_volume_settings(output: &str) -> Result<VolumeSnapshot> {
        let mut volume = None;
        let mut muted = None;

        for part in output.split(',') {
            let part = part.trim();
            if let Some(raw) = part.strip_prefix("output volume:") {
                let parsed = raw
                    .trim()
                    .parse::<f32>()
                    .context("failed to parse macOS output volume")?;
                volume = Some((parsed / 100.0).clamp(0.0, 1.0));
            } else if let Some(raw) = part.strip_prefix("output muted:") {
                let raw = raw.trim();
                muted = Some(match raw {
                    "true" => true,
                    "false" => false,
                    _ => return Err(anyhow!("failed to parse macOS output mute state: {raw}")),
                });
            }
        }

        Ok(VolumeSnapshot {
            volume: volume.ok_or_else(|| anyhow!("macOS output volume missing"))?,
            muted: muted.ok_or_else(|| anyhow!("macOS output mute state missing"))?,
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn parses_volume_settings() {
            let parsed = parse_volume_settings(
                "output volume:72, input volume:50, alert volume:100, output muted:false",
            )
            .expect("settings should parse");

            assert!((parsed.volume - 0.72).abs() < f32::EPSILON);
            assert!(!parsed.muted);
        }

        #[test]
        fn calculates_thirty_percent_of_current_volume() {
            assert!((ducked_volume(0.8) - 0.24).abs() < f32::EPSILON);
            assert!((ducked_volume(0.2) - 0.06).abs() < f32::EPSILON);
        }
    }
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use std::ptr;

    use super::*;
    use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::{
        eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
    };

    pub fn duck_to_30_percent_of_current() -> Result<Option<VolumeSnapshot>> {
        let _com = ComGuard::initialize()?;
        let endpoint = default_endpoint_volume()?;
        let current_volume = unsafe { endpoint.GetMasterVolumeLevelScalar() }
            .context("failed to read Windows output volume")?;
        let muted = unsafe { endpoint.GetMute() }
            .context("failed to read Windows output mute state")?
            .as_bool();

        if muted || current_volume <= 0.0 {
            return Ok(None);
        }

        unsafe {
            endpoint.SetMasterVolumeLevelScalar(ducked_volume(current_volume), ptr::null())?;
        }

        Ok(Some(VolumeSnapshot {
            volume: current_volume,
            muted,
        }))
    }

    pub fn restore(snapshot: VolumeSnapshot) -> Result<()> {
        let _com = ComGuard::initialize()?;
        let endpoint = default_endpoint_volume()?;
        unsafe {
            endpoint.SetMasterVolumeLevelScalar(snapshot.volume.clamp(0.0, 1.0), ptr::null())?;
            endpoint.SetMute(snapshot.muted, ptr::null())?;
        }
        Ok(())
    }

    fn default_endpoint_volume() -> Result<IAudioEndpointVolume> {
        let enumerator: IMMDeviceEnumerator =
            unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
                .context("failed to create Windows audio device enumerator")?;
        let device = unsafe { enumerator.GetDefaultAudioEndpoint(eRender, eConsole) }
            .context("failed to get Windows default output device")?;
        let endpoint = unsafe { device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) }
            .context("failed to activate Windows output volume endpoint")?;
        Ok(endpoint)
    }

    struct ComGuard {
        should_uninitialize: bool,
    }

    impl ComGuard {
        fn initialize() -> Result<Self> {
            let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
            if hr.is_ok() {
                return Ok(Self {
                    should_uninitialize: true,
                });
            }

            if hr == RPC_E_CHANGED_MODE {
                return Ok(Self {
                    should_uninitialize: false,
                });
            }

            hr.ok()
                .context("failed to initialize COM for volume control")?;
            Ok(Self {
                should_uninitialize: true,
            })
        }
    }

    impl Drop for ComGuard {
        fn drop(&mut self) {
            if self.should_uninitialize {
                unsafe {
                    CoUninitialize();
                }
            }
        }
    }
}
