//! What the host itself costs, for the once-a-second stats line: process CPU (percent of one core),
//! working set, and this process's GPU engine use by engine type (PDH `\GPU Engine(*)\Utilization
//! Percentage`, the counters Task Manager shows). Local only: nothing leaves the machine.

/// One reading.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reading {
    /// Percent of one core since the last reading (100 = one core busy).
    pub cpu_percent: f64,
    pub working_set_mb: f64,
    /// (engine type, percent) for this process, highest first: "3D", "VideoEncode",
    /// "VideoProcessing", "Copy". Empty where the counters are missing.
    pub gpu: Vec<(String, f64)>,
}

impl Reading {
    /// " | cpu 4.2% mem 38 MB gpu 3D 2% VideoEncode 18%".
    pub fn words(&self) -> String {
        let mut s = format!(" | cpu {:.1}% mem {:.0} MB", self.cpu_percent, self.working_set_mb);
        if !self.gpu.is_empty() {
            s.push_str(" gpu");
            for (e, p) in &self.gpu {
                s.push_str(&format!(" {e} {p:.0}%"));
            }
        }
        s
    }
}

/// The engine type in a `GPU Engine` instance name if it belongs to process `pid`:
/// "pid_1234_luid_0x00000000_0x0000F2C1_phys_0_eng_0_engtype_3D" → "3D".
pub fn engine_of(instance: &str, pid: u32) -> Option<&str> {
    let rest = instance.strip_prefix(&format!("pid_{pid}_"))?;
    let i = rest.find("engtype_")?;
    Some(&rest[i + "engtype_".len()..])
}

#[cfg(windows)]
pub use win::Meter;

#[cfg(not(windows))]
#[derive(Default)]
pub struct Meter;

#[cfg(not(windows))]
impl Meter {
    pub fn new() -> Meter {
        Meter
    }
    pub fn read(&mut self) -> Option<Reading> {
        None
    }
}

#[cfg(windows)]
mod win {
    use super::{engine_of, Reading};
    use std::collections::BTreeMap;
    use std::time::Instant;
    use windows::core::w;
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::System::Performance::{
        PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW, PdhOpenQueryW,
        PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA,
    };
    use windows::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows::Win32::System::Threading::{GetCurrentProcess, GetCurrentProcessId, GetProcessTimes};

    pub struct Meter {
        last: Option<(Instant, u64)>,
        query: Option<(PDH_HQUERY, PDH_HCOUNTER)>,
        pid: u32,
    }

    // SAFETY: PDH query and counter handles may be used from any thread (one at a time, which
    // `&mut self` ensures).
    unsafe impl Send for Meter {}

    fn ticks(f: FILETIME) -> u64 {
        (u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime)
    }

    impl Default for Meter {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Meter {
        pub fn new() -> Meter {
            let query = unsafe {
                let mut q = PDH_HQUERY::default();
                let mut c = PDH_HCOUNTER::default();
                if PdhOpenQueryW(None, 0, &mut q) == 0 {
                    if PdhAddEnglishCounterW(q, w!("\\GPU Engine(*)\\Utilization Percentage"), 0, &mut c) == 0 {
                        // A rate counter needs a first sample to measure from.
                        let _ = PdhCollectQueryData(q);
                        Some((q, c))
                    } else {
                        let _ = PdhCloseQuery(q);
                        None
                    }
                } else {
                    None
                }
            };
            Meter { last: None, query, pid: unsafe { GetCurrentProcessId() } }
        }

        fn cpu_ticks() -> Option<u64> {
            let (mut c, mut e, mut k, mut u) =
                (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
            unsafe { GetProcessTimes(GetCurrentProcess(), &mut c, &mut e, &mut k, &mut u).ok()? };
            Some(ticks(k) + ticks(u))
        }

        fn gpu(&mut self) -> Vec<(String, f64)> {
            let Some((q, c)) = self.query else { return Vec::new() };
            let mut by_engine: BTreeMap<String, f64> = BTreeMap::new();
            unsafe {
                if PdhCollectQueryData(q) != 0 {
                    return Vec::new();
                }
                let (mut size, mut count) = (0u32, 0u32);
                if PdhGetFormattedCounterArrayW(c, PDH_FMT_DOUBLE, &mut size, &mut count, None) != PDH_MORE_DATA {
                    return Vec::new();
                }
                // Items, then the instance-name strings they point into, in one buffer.
                let n = (size as usize).div_ceil(std::mem::size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>());
                let mut buf: Vec<PDH_FMT_COUNTERVALUE_ITEM_W> = vec![std::mem::zeroed(); n.max(1)];
                if PdhGetFormattedCounterArrayW(c, PDH_FMT_DOUBLE, &mut size, &mut count, Some(buf.as_mut_ptr())) != 0 {
                    return Vec::new();
                }
                for item in &buf[..count as usize] {
                    let Ok(name) = item.szName.to_string() else { continue };
                    if let Some(engine) = engine_of(&name, self.pid) {
                        *by_engine.entry(engine.to_string()).or_default() += item.FmtValue.Anonymous.doubleValue;
                    }
                }
            }
            let mut v: Vec<(String, f64)> = by_engine.into_iter().filter(|(_, p)| *p >= 0.5).collect();
            v.sort_by(|a, b| b.1.total_cmp(&a.1));
            v
        }

        /// A reading since the last one (`None` the first time, which only starts the clock).
        pub fn read(&mut self) -> Option<Reading> {
            let now = Instant::now();
            let cpu = Self::cpu_ticks()?;
            let gpu = self.gpu();
            let prev = self.last.replace((now, cpu));
            let (t0, c0) = prev?;
            let wall = now.duration_since(t0).as_secs_f64();
            // FILETIME ticks are 100 ns.
            let cpu_percent = if wall > 0.0 { (cpu.saturating_sub(c0) as f64 / 1e7) / wall * 100.0 } else { 0.0 };
            let mut m = PROCESS_MEMORY_COUNTERS {
                cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
                ..Default::default()
            };
            let ws = unsafe {
                if K32GetProcessMemoryInfo(GetCurrentProcess(), &mut m, m.cb).as_bool() {
                    m.WorkingSetSize as f64 / (1024.0 * 1024.0)
                } else {
                    0.0
                }
            };
            Some(Reading { cpu_percent, working_set_mb: ws, gpu })
        }
    }

    impl Drop for Meter {
        fn drop(&mut self) {
            if let Some((q, _)) = self.query {
                unsafe {
                    let _ = PdhCloseQuery(q);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engines_of_this_process_only() {
        let n = "pid_1234_luid_0x00000000_0x0000F2C1_phys_0_eng_3_engtype_VideoEncode";
        assert_eq!(engine_of(n, 1234), Some("VideoEncode"));
        assert_eq!(engine_of(n, 123), None);
        assert_eq!(engine_of(n, 12345), None);
        assert_eq!(engine_of("pid_1234_luid_x", 1234), None);
    }

    #[test]
    fn words() {
        let r = Reading {
            cpu_percent: 4.25,
            working_set_mb: 38.4,
            gpu: vec![("VideoEncode".into(), 18.2), ("3D".into(), 2.0)],
        };
        assert_eq!(r.words(), " | cpu 4.2% mem 38 MB gpu VideoEncode 18% 3D 2%");
        assert_eq!(Reading::default().words(), " | cpu 0.0% mem 0 MB");
    }
}
