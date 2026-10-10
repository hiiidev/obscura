//! D65 sRGB / Display P3 conversions at browser image boundaries. GL numeric
//! inputs and readPixels are never color-managed. Both spaces use the sRGB
//! transfer curve; the linear matrices follow CSS Color 4's D65 primaries.
use serde::{Deserialize,Serialize};

#[derive(Clone,Copy,Debug,Default,Deserialize,Serialize,PartialEq,Eq)]
pub enum ColorSpace {
    #[default]
    #[serde(rename="srgb")]
    Srgb,
    #[serde(rename="display-p3")]
    DisplayP3,
}
impl ColorSpace {
    pub fn parse(value: &str) -> Option<Self> {
        match value { "srgb"=>Some(Self::Srgb),"display-p3"=>Some(Self::DisplayP3),_=>None }
    }
}
fn linear(value: f64) -> f64 {
    if value.abs()<=0.04045 { value/12.92 }
    else { value.signum()*((value.abs()+0.055)/1.055).powf(2.4) }
}
fn encoded(value: f64) -> f64 {
    if value.abs()<=0.0031308 {value*12.92}
    else {value.signum()*(1.055*value.abs().powf(1.0/2.4)-0.055)}
}
fn multiply(matrix: [[f64;3];3], value: [f64;3]) -> [f64;3] {
    matrix.map(|row| row.iter().zip(value).map(|(a,b)|a*b).sum())
}
/// Preserve extended values for float uploads; clamp only when a fixed-point
/// destination is written. Alpha is independent of the RGB primaries.
pub(crate) fn convert(mut rgba: [f32;4], source: ColorSpace, target: ColorSpace) -> [f32;4] {
    if source==target { return rgba; }
    let rgb=[linear(rgba[0] as f64),linear(rgba[1] as f64),linear(rgba[2] as f64)];
    let xyz=multiply(match source {
        ColorSpace::Srgb=>[[506752.0/1228815.0,87881.0/245763.0,12673.0/70218.0],
            [87098.0/409605.0,175762.0/245763.0,12673.0/175545.0],
            [7918.0/409605.0,87881.0/737289.0,1001167.0/1053270.0]],
        ColorSpace::DisplayP3=>[[608311.0/1250200.0,189793.0/714400.0,198249.0/1000160.0],
            [35783.0/156275.0,247089.0/357200.0,198249.0/2500400.0],
            [0.0,32229.0/714400.0,5220557.0/5000800.0]],
    },rgb);
    let converted=multiply(match target {
        ColorSpace::Srgb=>[[12831.0/3959.0,-329.0/214.0,-1974.0/3959.0],
            [-851781.0/878810.0,1648619.0/878810.0,36519.0/878810.0],
            [705.0/12673.0,-2585.0/12673.0,705.0/667.0]],
        ColorSpace::DisplayP3=>[[446124.0/178915.0,-333277.0/357830.0,-72051.0/178915.0],
            [-14852.0/17905.0,63121.0/35810.0,423.0/17905.0],
            [11844.0/330415.0,-50337.0/660830.0,316169.0/330415.0]],
    },xyz);
    for channel in 0..3 { rgba[channel]=encoded(converted[channel]) as f32; }
    rgba
}
pub fn convert_rgba8(pixels: &mut [u8], source: ColorSpace, target: ColorSpace, premultiplied: bool) -> bool {
    if pixels.len()%4!=0 || pixels.len()>crate::pixels::MAX_TRANSFER_BYTES { return false; }
    if source==target { return true; }
    for pixel in pixels.chunks_exact_mut(4) {
        let alpha=pixel[3] as f32/255.0;
        let mut rgba=[pixel[0] as f32/255.0,pixel[1] as f32/255.0,pixel[2] as f32/255.0,alpha];
        if premultiplied { for value in &mut rgba[..3] { *value=if alpha>0.0 {*value/alpha} else {0.0}; } }
        rgba=convert(rgba,source,target);
        for channel in 0..3 {
            let value=rgba[channel].clamp(0.0,1.0)*if premultiplied {alpha} else {1.0};
            pixel[channel]=(value*255.0).round() as u8;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn spaces_accept_only_supported_webidl_values() {
        assert_eq!(ColorSpace::parse("srgb"),Some(ColorSpace::Srgb));
        assert_eq!(ColorSpace::parse("display-p3"),Some(ColorSpace::DisplayP3));
        for value in ["","SRGB","p3","display-p3 ","rec2020"] {assert_eq!(ColorSpace::parse(value),None);}
    }
    #[test]
    fn conversion_preserves_alpha_and_extended_primary_values() {
        let red=convert([1.0,0.0,0.0,0.25],ColorSpace::Srgb,ColorSpace::DisplayP3);
        for (actual,expected) in red.into_iter().zip([0.91748756,0.20028681,0.13856059,0.25]) {assert!((actual-expected).abs()<0.000002);}
        let wide=convert([1.0,0.0,0.0,0.25],ColorSpace::DisplayP3,ColorSpace::Srgb);
        assert!(wide[0]>1.0&&wide[1]<0.0&&wide[2]<0.0);assert_eq!(wide[3],0.25);
        let recovered=convert(wide,ColorSpace::Srgb,ColorSpace::DisplayP3);
        for (actual,expected) in recovered.into_iter().zip([1.0,0.0,0.0,0.25]) {assert!((actual-expected).abs()<0.000003);}
    }
    #[test]
    fn byte_conversion_is_identity_in_same_space_and_clips_only_at_destination() {
        let original=[1,7,19,0,255,0,0,255,128,128,128,128];let mut pixels=original;
        assert!(convert_rgba8(&mut pixels,ColorSpace::DisplayP3,ColorSpace::DisplayP3,true));assert_eq!(pixels,original);
        assert!(convert_rgba8(&mut pixels,ColorSpace::Srgb,ColorSpace::DisplayP3,false));assert_eq!(&pixels[4..8],&[234,51,35,255]);
        let mut wide=[255,0,0,127];assert!(convert_rgba8(&mut wide,ColorSpace::DisplayP3,ColorSpace::Srgb,false));assert_eq!(wide,[255,0,0,127]);
        assert!(!convert_rgba8(&mut [1,2,3],ColorSpace::Srgb,ColorSpace::Srgb,false));
        assert!(convert_rgba8(&mut [],ColorSpace::Srgb,ColorSpace::DisplayP3,false));
    }
    #[test]
    fn conversion_handles_transfer_thresholds_and_premultiplied_transparency() {
        for value in [-2.0,-0.04045,-0.001,0.0,0.001,0.04045,0.5,1.0,2.0] {
            assert!((encoded(linear(value))-value).abs()<0.0000001);
        }
        let mut pixels=[128,0,0,128,200,100,50,0];
        assert!(convert_rgba8(&mut pixels,ColorSpace::Srgb,ColorSpace::DisplayP3,true));
        assert_eq!(pixels,[117,26,18,128,0,0,0,0]);
    }
}
