//! Bounded RRG graph-buffer text import; no inferred frequency or credential.
#[derive(Clone, Debug, PartialEq)]
pub struct Signal {
    pub samples: Vec<i16>,
}
impl Signal {
    pub fn parse(text: &str) -> Result<Self, String> {
        if text.len() > 8 * 1024 * 1024 {
            return Err("Signal file exceeds 8 MiB".into());
        }
        let mut samples = Vec::new();
        for (index, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let value = line.parse::<i16>().map_err(|_| {
                format!("Line {} must contain one signed decimal sample", index + 1)
            })?;
            if !(-127..=127).contains(&value) {
                return Err(format!(
                    "Line {} is outside RRG's -127..127 range",
                    index + 1
                ));
            }
            samples.push(value);
            if samples.len() > 1_000_000 {
                return Err("Signal exceeds one million samples".into());
            }
        }
        if samples.len() < 2 {
            return Err("A signal needs at least two samples".into());
        }
        Ok(Self { samples })
    }
    /// Min/max per pixel keeps short peaks visible when zoomed out.
    pub fn envelope(
        &self,
        start: usize,
        end: usize,
        pixels: usize,
    ) -> Result<Vec<(i16, i16)>, String> {
        if start >= end || end > self.samples.len() || pixels == 0 || pixels > 8192 {
            return Err("Invalid signal viewport".into());
        }
        let n = (end - start).min(pixels);
        Ok((0..n)
            .map(|i| {
                let a = start + i * (end - start) / n;
                let b = start + (i + 1) * (end - start) / n;
                let s = &self.samples[a..b];
                (*s.iter().min().unwrap(), *s.iter().max().unwrap())
            })
            .collect())
    }
}
