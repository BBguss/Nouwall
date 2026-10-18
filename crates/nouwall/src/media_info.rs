//! Read resolution, frame rate and codec of a video file through Media Foundation.

use std::path::Path;

use nouwall_core::{Codec, VideoMeta};
use windows::core::GUID;
use windows::Win32::Media::MediaFoundation::*;

use crate::util::hstring;

pub fn startup() {
    unsafe {
        let _ = MFStartup(MF_VERSION, MFSTARTUP_FULL);
    }
}

fn codec_from_subtype(g: GUID) -> Option<Codec> {
    if g == MFVideoFormat_H264 || g == MFVideoFormat_H264_ES {
        Some(Codec::H264)
    } else if g == MFVideoFormat_HEVC || g == MFVideoFormat_HEVC_ES {
        Some(Codec::Hevc)
    } else if g == MFVideoFormat_VP90 {
        Some(Codec::Vp9)
    } else if g == MFVideoFormat_AV1 {
        Some(Codec::Av1)
    } else {
        None
    }
}

pub fn read(path: &Path) -> windows::core::Result<VideoMeta> {
    unsafe {
        let reader = MFCreateSourceReaderFromURL(&hstring(&path.to_string_lossy()), None)?;
        let ty = reader.GetNativeMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32, 0)?;
        let size = ty.GetUINT64(&MF_MT_FRAME_SIZE)?;
        let (width, height) = ((size >> 32) as u32, size as u32);
        let fps = ty
            .GetUINT64(&MF_MT_FRAME_RATE)
            .map(|r| {
                let (num, den) = ((r >> 32) as u32, (r as u32).max(1));
                (num as f64 / den as f64).round() as u32
            })
            .unwrap_or(0);
        let codec = ty.GetGUID(&MF_MT_SUBTYPE).ok().and_then(codec_from_subtype);
        Ok(VideoMeta { width, height, fps, codec })
    }
}
