pub struct TestTone {
    rate: u32,
    position: u64,
}

impl TestTone {
    pub fn new(rate: u32) -> Self {
        Self { rate, position: 0 }
    }

    pub fn read(&mut self, frames: usize, out: &mut Vec<f32>) {
        out.clear();
        let rate = self.rate as f64;
        for frame in 0..frames {
            let t = (self.position + frame as u64) as f64 / rate;
            let beat = t % 0.5;
            let kick =
                (std::f64::consts::TAU * (55.0 * beat + 60.0 * (1.0 - (-beat * 25.0).exp()))).sin()
                    * (-beat * 8.0).exp()
                    * 0.55;
            let bass = (std::f64::consts::TAU * 110.0 * t).sin() * 0.18;
            let pad = (std::f64::consts::TAU * 220.0 * t).sin() * 0.1
                + (std::f64::consts::TAU * 329.63 * t).sin() * 0.07;
            let sweep = ((t * 0.15).sin() * 3000.0 + 5000.0).max(200.0);
            let noise = (std::f64::consts::TAU * sweep * t).sin() * 0.025;
            let sample = (kick + bass + pad + noise) as f32;
            out.push(sample);
        }
        self.position += frames as u64;
    }
}
