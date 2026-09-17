use super::*;

#[derive(Clone)]
pub(super) enum Space {
    Device(usize),
    Indexed(Vec<[u8; 3]>),
    Cal(Box<Calibrated>),
    Icc(Box<moxcms::ColorProfile>, usize),
}
impl Space {
    pub(super) fn equivalent(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Device(a), Self::Device(b)) => a == b,
            (Self::Icc(a, n), Self::Icc(b, m)) => n == m && a.encode().ok().zip(b.encode().ok()).is_some_and(|(a, b)| a == b),
            _ => false,
        }
    }
    pub(super) fn channels(&self) -> usize {
        match self {
            Self::Device(n) | Self::Icc(_, n) => *n,
            Self::Indexed(_) => 1,
            Self::Cal(cal) => {
                if cal.kind == 0 {
                    1
                } else {
                    3
                }
            }
        }
    }
    pub(super) fn map_samples(&self, samples: &mut [u8], bits: i64, mapping: Option<&[f64]>) -> Option<()> {
        if matches!(self, Self::Indexed(_)) && bits > 8 {
            return None;
        }
        let defaults: Vec<f64> = match self {
            Self::Indexed(_) => vec![0., ((1_u32 << bits) - 1) as f64],
            Self::Cal(cal) if cal.kind == 2 => vec![0., 100., cal.range[0], cal.range[1], cal.range[2], cal.range[3]],
            _ => (0..self.channels()).flat_map(|_| [0., 1.]).collect(),
        };
        let mapping = mapping.unwrap_or(&defaults);
        if mapping.len() != self.channels() * 2 {
            return None;
        }
        for pixel in samples.chunks_exact_mut(self.channels()) {
            for (i, sample) in pixel.iter_mut().enumerate() {
                let value = mapping[2 * i] + f64::from(*sample) / 255. * (mapping[2 * i + 1] - mapping[2 * i]);
                let encoded = if matches!(self, Self::Indexed(_)) { value } else { (value - defaults[2 * i]) / (defaults[2 * i + 1] - defaults[2 * i]) * 255. };
                *sample = encoded.round().clamp(0., 255.) as u8;
            }
        }
        Some(())
    }
    pub(super) fn icc(bytes: &[u8], channels: usize) -> Option<Self> {
        if bytes.len() > 4 * 1024 * 1024 {
            return None;
        }
        let profile = moxcms::ColorProfile::new_from_slice(bytes).ok()?;
        let count = match profile.color_space {
            moxcms::DataColorSpace::Gray => 1,
            moxcms::DataColorSpace::Rgb => 3,
            moxcms::DataColorSpace::Cmyk => 4,
            _ => return None,
        };
        (channels == count).then_some(Self::Icc(Box::new(profile), channels))
    }
    pub(super) fn rgb(&self, samples: Vec<u8>, width: u32, height: u32) -> Option<RgbImage> {
        let pixels = usize::try_from(u64::from(width) * u64::from(height)).ok()?;
        if samples.len() != pixels.checked_mul(self.channels())? {
            return None;
        }
        match self {
            Self::Indexed(palette) => {
                let mut rgb = Vec::with_capacity(pixels * 3);
                for index in samples {
                    rgb.extend_from_slice(palette.get(usize::from(index).min(palette.len() - 1))?);
                }
                RgbImage::from_raw(width, height, rgb)
            }
            Self::Cal(cal) => {
                let mut rgb = Vec::with_capacity(pixels * 3);
                for pixel in samples.chunks_exact(self.channels()) {
                    rgb.extend_from_slice(&cal.rgb(pixel));
                }
                RgbImage::from_raw(width, height, rgb)
            }
            Self::Icc(profile, n) => {
                let layout = match n {
                    1 => moxcms::Layout::Gray,
                    3 => moxcms::Layout::Rgb,
                    4 => moxcms::Layout::Rgba,
                    _ => return None,
                };
                let transform = profile.create_transform_8bit(layout, &moxcms::ColorProfile::new_srgb(), moxcms::Layout::Rgb, Default::default()).ok()?;
                let mut rgb = vec![0; pixels.checked_mul(3)?];
                transform.transform(&samples, &mut rgb).ok()?;
                RgbImage::from_raw(width, height, rgb)
            }
            Self::Device(3) => RgbImage::from_raw(width, height, samples),
            Self::Device(1) => Some(DynamicImage::ImageLuma8(image::GrayImage::from_raw(width, height, samples)?).to_rgb8()),
            Self::Device(4) => {
                let mut rgb = Vec::with_capacity(pixels.checked_mul(3)?);
                for pixel in samples.chunks_exact(4) {
                    for channel in &pixel[..3] {
                        rgb.push(((255 - u16::from(*channel)) * (255 - u16::from(pixel[3])) / 255) as u8);
                    }
                }
                RgbImage::from_raw(width, height, rgb)
            }
            _ => None,
        }
    }
}

pub(super) fn resolve_space(document: &Document, resources: &Dictionary, object: &Object) -> Option<Space> {
    resolve_inner(document, resources, object, 0)
}
fn resolve_inner(document: &Document, resources: &Dictionary, object: &Object, depth: usize) -> Option<Space> {
    if depth > 8 {
        return None;
    }
    let mut object = object;
    let mut use_default = true;
    for _ in 0..32 {
        object = resolve(document, object)?;
        if let Ok(array) = object.as_array() {
            match array.first()?.as_name().ok()? {
                b"Indexed" if array.len() == 4 => {
                    let base = resolve_inner(document, resources, &array[1], depth + 1)?;
                    if matches!(base, Space::Indexed(_)) {
                        return None;
                    }
                    let high = resolve(document, &array[2])?.as_i64().ok()?;
                    if !(0..=255).contains(&high) {
                        return None;
                    }
                    let lookup = resolve(document, &array[3])?;
                    let bytes = match lookup {
                        Object::String(bytes, _) => bytes.clone(),
                        Object::Stream(stream) => stream.get_plain_content_with_limit(1024).ok()?,
                        _ => return None,
                    };
                    let length = (high as usize + 1) * base.channels();
                    let rgb = base.rgb(bytes.get(..length)?.to_vec(), high as u32 + 1, 1)?;
                    return Some(Space::Indexed(rgb.pixels().map(|p| p.0).collect()));
                }
                b"CalGray" | b"CalRGB" | b"Lab" if array.len() == 2 => {
                    return Some(Space::Cal(Box::new(Calibrated::parse(document, array[0].as_name().ok()?, resolve(document, &array[1])?.as_dict().ok()?)?)));
                }
                _ => (),
            }
            if array.len() != 2 || array[0].as_name().ok()? != b"ICCBased" {
                return None;
            }
            let profile = resolve(document, &array[1])?.as_stream().ok()?;
            let n = usize::try_from(profile.dict.get(b"N").ok()?.as_i64().ok()?).ok()?;
            return Space::icc(&profile.get_plain_content_with_limit(4 * 1024 * 1024).ok()?, n);
        }
        let name = object.as_name().ok()?;
        let device = match name {
            b"DeviceGray" => Some((1, b"DefaultGray".as_slice())),
            b"DeviceRGB" => Some((3, b"DefaultRGB".as_slice())),
            b"DeviceCMYK" => Some((4, b"DefaultCMYK".as_slice())),
            _ => None,
        };
        let spaces = match resources.get(b"ColorSpace") {
            Ok(value) => Some(resolve(document, value)?.as_dict().ok()?),
            Err(_) => None,
        };
        if let Some((n, default)) = device {
            if use_default {
                use_default = false;
                if let Some(value) = spaces.and_then(|s| s.get(default).ok()) {
                    object = value;
                    continue;
                }
            }
            return Some(Space::Device(n));
        }
        object = spaces?.get(name).ok()?;
    }
    None
}

#[derive(Clone)]
pub(super) struct Calibrated {
    kind: u8,
    white: [f64; 3],
    gamma: [f64; 3],
    matrix: [f64; 9],
    range: [f64; 4],
}
impl Calibrated {
    fn parse(document: &Document, name: &[u8], dict: &Dictionary) -> Option<Self> {
        let white: [f64; 3] = numbers(resolve(document, dict.get(b"WhitePoint").ok()?)?.as_array().ok()?)?.try_into().ok()?;
        if white[0] <= 0. || white[1] != 1. || white[2] <= 0. {
            return None;
        }
        if let Ok(value) = dict.get(b"BlackPoint") {
            if numbers(resolve(document, value)?.as_array().ok()?)? != [0., 0., 0.] {
                return None;
            }
        }
        let kind = match name {
            b"CalGray" => 0,
            b"CalRGB" => 1,
            b"Lab" => 2,
            _ => return None,
        };
        let mut gamma = [1.; 3];
        if let Ok(value) = dict.get(b"Gamma") {
            let value = resolve(document, value)?;
            gamma = if kind == 0 { [number(value)?; 3] } else { numbers(value.as_array().ok()?)?.try_into().ok()? };
            if gamma.iter().any(|v| *v <= 0.) {
                return None;
            }
        }
        let matrix = match dict.get(b"Matrix") {
            Ok(value) => numbers(resolve(document, value)?.as_array().ok()?)?.try_into().ok()?,
            Err(_) => [1., 0., 0., 0., 1., 0., 0., 0., 1.],
        };
        let range = match dict.get(b"Range") {
            Ok(value) => numbers(resolve(document, value)?.as_array().ok()?)?.try_into().ok()?,
            Err(_) => [-100., 100., -100., 100.],
        };
        if range[0] >= range[1] || range[2] >= range[3] {
            return None;
        }
        Some(Self { kind, white, gamma, matrix, range })
    }
    fn rgb(&self, p: &[u8]) -> [u8; 3] {
        let xyz = match self.kind {
            0 => self.white.map(|v| v * (f64::from(p[0]) / 255.).powf(self.gamma[0])),
            1 => {
                let a: [f64; 3] = std::array::from_fn(|i| (f64::from(p[i]) / 255.).powf(self.gamma[i]));
                std::array::from_fn(|i| self.matrix[i] * a[0] + self.matrix[i + 3] * a[1] + self.matrix[i + 6] * a[2])
            }
            _ => {
                let l = f64::from(p[0]) / 255. * 100.;
                let a = self.range[0] + f64::from(p[1]) / 255. * (self.range[1] - self.range[0]);
                let b = self.range[2] + f64::from(p[2]) / 255. * (self.range[3] - self.range[2]);
                let f = |v: f64| if v >= 6. / 29. { v * v * v } else { 3. * (6_f64 / 29.).powi(2) * (v - 4. / 29.) };
                let y = (l + 16.) / 116.;
                [self.white[0] * f(y + a / 500.), f(y), self.white[2] * f(y - b / 200.)]
            }
        };
        // Bradford chromatic adaptation from the declared white to sRGB D65.
        let cone = |v: [f64; 3]| [0.8951 * v[0] + 0.2664 * v[1] - 0.1614 * v[2], -0.7502 * v[0] + 1.7135 * v[1] + 0.0367 * v[2], 0.0389 * v[0] - 0.0685 * v[1] + 1.0296 * v[2]];
        let source = cone(self.white);
        let target = cone([0.95047, 1., 1.08883]);
        let mut adapted = cone(xyz);
        for i in 0..3 {
            adapted[i] *= target[i] / source[i];
        }
        let [a, b, c] = adapted;
        let x = 0.9869929 * a - 0.1470543 * b + 0.1599627 * c;
        let y = 0.4323053 * a + 0.5183603 * b + 0.0492912 * c;
        let z = -0.0085287 * a + 0.0400428 * b + 0.9684867 * c;
        [3.2404542 * x - 1.5371385 * y - 0.4985314 * z, -0.969266 * x + 1.8760108 * y + 0.041556 * z, 0.0556434 * x - 0.2040259 * y + 1.0572252 * z].map(|v| {
            let v = if v <= 0.0031308 { 12.92 * v } else { 1.055 * v.powf(1. / 2.4) - 0.055 };
            (v * 255.).round().clamp(0., 255.) as u8
        })
    }
}
