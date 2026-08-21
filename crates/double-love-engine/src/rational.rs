//! Rational Time 基准：词 = 采样整数，剪辑/导出 = 帧整数。
//! f64 秒只允许出现在 UI 显示层，禁止作为剪辑基准（PRD 不变量）。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 有理数（构造即约分，分母恒正）。禁止从 f64 构造。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Rational {
    pub num: i64,
    pub den: i64,
}

impl Rational {
    pub fn new(num: i64, den: i64) -> Self {
        assert!(den != 0, "rational denominator must not be zero");
        let divisor = gcd(num.unsigned_abs(), den.unsigned_abs()) as i64;
        let sign = if den < 0 { -1 } else { 1 };
        Self {
            num: sign * num / divisor,
            den: den.abs() / divisor,
        }
    }

    /// 解析 "30000/1001" 形式；任何尾随字符都视为非法（DL-021 教训）。
    pub fn parse(text: &str) -> Option<Self> {
        let (num, den) = text.split_once('/')?;
        let num: i64 = num.trim().parse().ok()?;
        let den: i64 = den.trim().parse().ok()?;
        if den == 0 {
            return None;
        }
        Some(Self::new(num, den))
    }
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 { a.max(1) } else { gcd(b, a % b) }
}

/// 帧率白名单：切片只支持这些；ffprobe 探测结果必须精确命中其一。
/// serde 名显式固定（外部契约，会存进 SQLite），不依赖 rename_all 的分词行为。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum FrameRate {
    #[serde(rename = "fps_24")]
    Fps24,
    #[serde(rename = "fps_24_ntsc")]
    Fps24Ntsc,
    #[serde(rename = "fps_25")]
    Fps25,
    #[serde(rename = "fps_30")]
    Fps30,
    #[serde(rename = "fps_30_ntsc")]
    Fps30Ntsc,
    #[serde(rename = "fps_50")]
    Fps50,
    #[serde(rename = "fps_60")]
    Fps60,
    #[serde(rename = "fps_60_ntsc")]
    Fps60Ntsc,
}

impl FrameRate {
    /// 精确有理帧率（如 24000/1001）。
    pub fn rational(self) -> Rational {
        match self {
            Self::Fps24 => Rational::new(24, 1),
            Self::Fps24Ntsc => Rational::new(24000, 1001),
            Self::Fps25 => Rational::new(25, 1),
            Self::Fps30 => Rational::new(30, 1),
            Self::Fps30Ntsc => Rational::new(30000, 1001),
            Self::Fps50 => Rational::new(50, 1),
            Self::Fps60 => Rational::new(60, 1),
            Self::Fps60Ntsc => Rational::new(60000, 1001),
        }
    }

    /// XMEML `<timebase>`：名义整数帧率。
    pub fn timebase(self) -> i64 {
        match self {
            Self::Fps24 | Self::Fps24Ntsc => 24,
            Self::Fps25 => 25,
            Self::Fps30 | Self::Fps30Ntsc => 30,
            Self::Fps50 => 50,
            Self::Fps60 | Self::Fps60Ntsc => 60,
        }
    }

    /// XMEML `<ntsc>`：0.1% 变速帧率必须标 TRUE（DL-018）。
    pub fn is_ntsc(self) -> bool {
        matches!(self, Self::Fps24Ntsc | Self::Fps30Ntsc | Self::Fps60Ntsc)
    }

    /// 从有理帧率反查白名单；未命中返回 None（调用方给出 MEDIA_FPS_UNSUPPORTED）。
    pub fn from_rational(rate: &Rational) -> Option<Self> {
        const ALL: [FrameRate; 8] = [
            FrameRate::Fps24,
            FrameRate::Fps24Ntsc,
            FrameRate::Fps25,
            FrameRate::Fps30,
            FrameRate::Fps30Ntsc,
            FrameRate::Fps50,
            FrameRate::Fps60,
            FrameRate::Fps60Ntsc,
        ];
        ALL.into_iter()
            .find(|candidate| candidate.rational() == *rate)
    }
}

/// 采样→帧量化方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Round {
    Floor,
    Ceil,
}

/// 源帧号转换到另一条时间基。时间线拼接只在这里做跨帧率换算，避免 UI 或导出器
/// 各自用浮点秒重复计算。
pub fn convert_frame_rate(frame: i64, source: FrameRate, output: FrameRate, round: Round) -> i64 {
    debug_assert!(frame >= 0);
    let source_fps = source.rational();
    let output_fps = output.rational();
    let num = frame as i128 * source_fps.den as i128 * output_fps.num as i128;
    let den = source_fps.num as i128 * output_fps.den as i128;
    let base = num.div_euclid(den);
    match round {
        Round::Floor => base as i64,
        Round::Ceil if num.rem_euclid(den) == 0 => base as i64,
        Round::Ceil => (base + 1) as i64,
    }
}

/// 采样数 → 帧号（整数运算，i128 防溢出）。
pub fn samples_to_frame(sample: i64, rate: FrameRate, sample_rate: i64, round: Round) -> i64 {
    debug_assert!(sample >= 0 && sample_rate > 0);
    let fps = rate.rational();
    let num = sample as i128 * fps.num as i128;
    let den = fps.den as i128 * sample_rate as i128;
    let base = num.div_euclid(den);
    let value = match round {
        Round::Floor => base,
        Round::Ceil => {
            if num.rem_euclid(den) == 0 {
                base
            } else {
                base + 1
            }
        }
    };
    value as i64
}

/// 帧号 → 采样数（floor；与 samples_to_frame 构成边界可测的往返）。
pub fn frame_to_samples(frame: i64, rate: FrameRate, sample_rate: i64) -> i64 {
    debug_assert!(frame >= 0 && sample_rate > 0);
    let fps = rate.rational();
    (frame as i128 * fps.den as i128 * sample_rate as i128).div_euclid(fps.num as i128) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rational_reduces_on_construction() {
        assert_eq!(Rational::new(48, 2), Rational { num: 24, den: 1 });
        assert_eq!(
            Rational::new(30000, 1001),
            Rational {
                num: 30000,
                den: 1001
            }
        );
        assert_eq!(Rational::new(1, -2), Rational { num: -1, den: 2 });
    }

    #[test]
    fn rational_parse_is_strict() {
        assert_eq!(
            Rational::parse("30000/1001"),
            Some(Rational::new(30000, 1001))
        );
        assert_eq!(Rational::parse("25/1extra"), None);
        assert_eq!(Rational::parse("25"), None);
        assert_eq!(Rational::parse("0/0"), None);
    }

    #[test]
    fn frame_rate_mapping_matches_xmeml_contract() {
        assert_eq!(FrameRate::Fps24Ntsc.timebase(), 24);
        assert!(FrameRate::Fps24Ntsc.is_ntsc());
        assert_eq!(FrameRate::Fps30Ntsc.timebase(), 30);
        assert!(FrameRate::Fps30Ntsc.is_ntsc());
        assert_eq!(FrameRate::Fps60Ntsc.timebase(), 60);
        assert!(FrameRate::Fps60Ntsc.is_ntsc());
        assert_eq!(FrameRate::Fps25.timebase(), 25);
        assert!(!FrameRate::Fps25.is_ntsc());
    }

    #[test]
    fn frame_rate_round_trip_from_rational() {
        for rate in [
            FrameRate::Fps24,
            FrameRate::Fps24Ntsc,
            FrameRate::Fps25,
            FrameRate::Fps30,
            FrameRate::Fps30Ntsc,
            FrameRate::Fps50,
            FrameRate::Fps60,
            FrameRate::Fps60Ntsc,
        ] {
            assert_eq!(FrameRate::from_rational(&rate.rational()), Some(rate));
        }
        assert_eq!(FrameRate::from_rational(&Rational::new(15, 1)), None);
    }

    #[test]
    fn samples_to_frame_exact_boundaries() {
        // 25fps @ 48kHz：每帧 1920 采样
        assert_eq!(
            samples_to_frame(0, FrameRate::Fps25, 48000, Round::Floor),
            0
        );
        assert_eq!(
            samples_to_frame(1920, FrameRate::Fps25, 48000, Round::Floor),
            1
        );
        assert_eq!(
            samples_to_frame(1919, FrameRate::Fps25, 48000, Round::Floor),
            0
        );
        assert_eq!(
            samples_to_frame(1919, FrameRate::Fps25, 48000, Round::Ceil),
            1
        );
        // 23.976 @ 48kHz：每帧 2002 采样（48000*1001/24000 = 2002，整数）
        assert_eq!(
            samples_to_frame(2002, FrameRate::Fps24Ntsc, 48000, Round::Floor),
            1
        );
        assert_eq!(
            samples_to_frame(2001, FrameRate::Fps24Ntsc, 48000, Round::Ceil),
            1
        );
    }

    #[test]
    fn frame_samples_round_trip() {
        for frame in [0, 1, 25, 1000, 1229156] {
            let samples = frame_to_samples(frame, FrameRate::Fps25, 48000);
            assert_eq!(
                samples_to_frame(samples, FrameRate::Fps25, 48000, Round::Floor),
                frame
            );
        }
    }

    #[test]
    fn converts_frame_counts_without_float_seconds() {
        // 25 帧 @ 25fps 恰好是一秒，输出到 29.97fps 是 29.97 帧。
        assert_eq!(
            convert_frame_rate(25, FrameRate::Fps25, FrameRate::Fps30Ntsc, Round::Floor),
            29
        );
        assert_eq!(
            convert_frame_rate(25, FrameRate::Fps25, FrameRate::Fps30Ntsc, Round::Ceil),
            30
        );
        assert_eq!(
            convert_frame_rate(24, FrameRate::Fps24, FrameRate::Fps24Ntsc, Round::Floor),
            23
        );
    }
}
