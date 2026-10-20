//! The monitor set, as far as floating panels care (f101, A013): a name and a rectangle in
//! screen points for each monitor, the first one the primary. Pure maths: where a window
//! belongs, and how to put one back inside a monitor.

/// One monitor: a name (letters, digits, `.` and `-` only) and its place on the desktop.
#[derive(Debug, Clone, PartialEq)]
pub struct Monitor {
    pub name: String,
    pub pos: [f32; 2],
    pub size: [f32; 2],
}

/// A name that is safe inside the layout text (which splits on `_ , ; @ :`).
pub fn clean_name(raw: &str) -> String {
    let s: String = raw.trim().chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '-' }).collect();
    if s.is_empty() { "monitor".to_string() } else { s }
}

/// `name:x,y,w,h;name:x,y,w,h` (the first is the primary). Used by the `MONITORS simulate` verb
/// and the `WAVCRAFT_MONITORS` variable.
pub fn parse(text: &str) -> Result<Vec<Monitor>, String> {
    const USAGE: &str = "MONITORS simulate <name>:<x>,<y>,<w>,<h>[;<name>:…] (the first is the primary)";
    let mut out: Vec<Monitor> = Vec::new();
    for part in text.split(';').map(str::trim).filter(|p| !p.is_empty()) {
        let (name, nums) = part.split_once(':').ok_or(USAGE)?;
        let n: Vec<f32> = nums.split(',').map(|v| v.trim().parse::<f32>()).collect::<Result<_, _>>().map_err(|_| USAGE.to_string())?;
        let [x, y, w, h] = n[..] else { return Err(USAGE.into()) };
        if !n.iter().all(|v| v.is_finite()) || w < 1.0 || h < 1.0 {
            return Err(USAGE.into());
        }
        let name = clean_name(name);
        if out.iter().any(|m| m.name == name) {
            return Err(format!("two monitors are named {name:?}"));
        }
        out.push(Monitor { name, pos: [x, y], size: [w, h] });
    }
    if out.is_empty() { Err(USAGE.into()) } else { Ok(out) }
}

/// The monitor a point is on; when it is on none, the nearest one.
pub fn monitor_at(mons: &[Monitor], p: [f32; 2]) -> Option<&Monitor> {
    let dist = |m: &Monitor| {
        let dx = (m.pos[0] - p[0]).max(0.0).max(p[0] - (m.pos[0] + m.size[0]));
        let dy = (m.pos[1] - p[1]).max(0.0).max(p[1] - (m.pos[1] + m.size[1]));
        dx * dx + dy * dy
    };
    mons.iter().min_by(|a, b| dist(a).total_cmp(&dist(b)))
}

/// A window rectangle moved (and, if it is bigger than the monitor, shrunk) until it lies wholly
/// inside the monitor. Returns (position, size), rounded to whole points.
pub fn clamp_into(pos: [f32; 2], size: [f32; 2], m: &Monitor) -> ([f32; 2], [f32; 2]) {
    let mut p = [0.0; 2];
    let mut s = [0.0; 2];
    for i in 0..2 {
        s[i] = size[i].min(m.size[i]).round();
        p[i] = pos[i].clamp(m.pos[i], m.pos[i] + m.size[i] - s[i]).round();
    }
    (p, s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two() -> Vec<Monitor> {
        parse("main:0,0,1920,1080;side:1920,0,1280,1024").unwrap()
    }

    #[test]
    fn parsing_reads_names_and_rects_and_refuses_nonsense() {
        let m = two();
        assert_eq!(m.len(), 2);
        assert_eq!((m[0].name.as_str(), m[1].pos, m[1].size), ("main", [1920.0, 0.0], [1280.0, 1024.0]));
        assert_eq!(parse("My Screen:0,0,10,10").unwrap()[0].name, "My-Screen");
        for bad in ["", "a", "a:1,2,3", "a:0,0,0,5", "a:0,0,x,5", "a:0,0,5,5;a:9,0,5,5"] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_point_belongs_to_the_monitor_it_is_on_or_the_nearest() {
        let m = two();
        assert_eq!(monitor_at(&m, [100.0, 100.0]).unwrap().name, "main");
        assert_eq!(monitor_at(&m, [2000.0, 100.0]).unwrap().name, "side");
        assert_eq!(monitor_at(&m, [5000.0, 50.0]).unwrap().name, "side");
        assert_eq!(monitor_at(&m, [-300.0, -50.0]).unwrap().name, "main");
        assert!(monitor_at(&[], [0.0, 0.0]).is_none());
    }

    #[test]
    fn clamping_moves_a_window_inside_and_shrinks_one_that_is_too_big() {
        let m = &two()[0];
        assert_eq!(clamp_into([2000.0, 100.0], [320.0, 260.0], m), ([1600.0, 100.0], [320.0, 260.0]));
        assert_eq!(clamp_into([-50.0, -9.0], [320.0, 260.0], m), ([0.0, 0.0], [320.0, 260.0]));
        assert_eq!(clamp_into([10.0, 1000.0], [320.0, 260.0], m), ([10.0, 820.0], [320.0, 260.0]));
        assert_eq!(clamp_into([500.0, 500.0], [4000.0, 3000.0], m), ([0.0, 0.0], [1920.0, 1080.0]));
        assert_eq!(clamp_into([100.0, 100.0], [320.0, 260.0], m), ([100.0, 100.0], [320.0, 260.0]));
        let off = Monitor { name: "o".into(), pos: [-1000.0, 200.0], size: [800.0, 600.0] };
        assert_eq!(clamp_into([0.0, 0.0], [320.0, 260.0], &off), ([-520.0, 200.0], [320.0, 260.0]));
    }
}
