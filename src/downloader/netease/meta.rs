// 音频元数据写入 (统一 MP3 / FLAC / M4A)
//   write_tags(audio_path, detail, cover_bytes)
//     - 自动按文件扩展名选 TagType (mp3 → Id3v2, flac → VorbisComments, m4a → Mp4Ilst)
//     - 设置 title / artist / album
//     - 嵌入封面图 (APIC frame for mp3, PICTURE block for flac)

use std::path::Path;

use lofty::config::WriteOptions;
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::tag::{Accessor, Tag, TagExt, TagType};

use crate::downloader::netease::api::SongDetailItem;
use crate::error::DownloadError;

pub fn write_tags(
    audio_path: &Path,
    detail: &SongDetailItem,
    cover: Option<&[u8]>,
) -> Result<(), DownloadError> {
    let tag_type = pick_tag_type(audio_path);
    let mut tag = Tag::new(tag_type);

    tag.set_title(detail.name.clone());

    let artists = detail
        .ar
        .iter()
        .map(|a| a.name.as_str())
        .collect::<Vec<_>>()
        .join(" / ");
    if !artists.is_empty() {
        tag.set_artist(artists);
    }

    if !detail.al.name.is_empty() {
        tag.set_album(detail.al.name.clone());
    }

    if let Some(cover_bytes) = cover
        && !cover_bytes.is_empty()
    {
        let mime = sniff_image_mime(cover_bytes);
        let pic = Picture::new_unchecked(
            PictureType::CoverFront,
            Some(mime),
            None,
            cover_bytes.to_vec(),
        );
        tag.push_picture(pic);
    }

    tag.save_to_path(audio_path, WriteOptions::default())
        .map_err(|e| DownloadError::Other(format!("写元数据失败: {e}")))?;

    Ok(())
}

// 根据扩展名选 TagType
//   不依赖 lofty 自动探测,因为我们已经知道扩展名
fn pick_tag_type(path: &Path) -> TagType {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("flac") => TagType::VorbisComments,
        Some("m4a") | Some("mp4") => TagType::Mp4Ilst,
        Some("ogg") | Some("opus") => TagType::VorbisComments,
        _ => TagType::Id3v2,
    }
}

// 嗅探图片 MIME (网易云封面通常 jpg,但偶尔 png/webp)
//   只看头几字节 magic bytes,跨平台 0 IO
fn sniff_image_mime(bytes: &[u8]) -> MimeType {
    if bytes.len() >= 3 && &bytes[..3] == b"\xff\xd8\xff" {
        MimeType::Jpeg
    } else if bytes.len() >= 8 && &bytes[..8] == b"\x89PNG\r\n\x1a\n" {
        MimeType::Png
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        MimeType::Unknown("image/webp".into())
    } else {
        MimeType::Jpeg
    }
}

// 根据 song_url_v1 返回的 type 字段决定输出扩展名
pub fn ext_for_type(audio_type: Option<&str>) -> &'static str {
    match audio_type.map(str::to_ascii_lowercase).as_deref() {
        Some("flac") => "flac",
        Some("m4a") => "m4a",
        Some("ogg") => "ogg",
        _ => "mp3",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_tag_type_by_ext() {
        assert_eq!(pick_tag_type(Path::new("a.mp3")), TagType::Id3v2);
        assert_eq!(pick_tag_type(Path::new("a.FLAC")), TagType::VorbisComments);
        assert_eq!(pick_tag_type(Path::new("a.m4a")), TagType::Mp4Ilst);
        assert_eq!(pick_tag_type(Path::new("a.ogg")), TagType::VorbisComments);
        assert_eq!(pick_tag_type(Path::new("noext")), TagType::Id3v2);
    }

    #[test]
    fn sniff_jpeg_magic() {
        let jpeg = [0xff, 0xd8, 0xff, 0xe0];
        assert!(matches!(sniff_image_mime(&jpeg), MimeType::Jpeg));
    }

    #[test]
    fn sniff_png_magic() {
        let png = b"\x89PNG\r\n\x1a\nIHDR";
        assert!(matches!(sniff_image_mime(png), MimeType::Png));
    }

    #[test]
    fn ext_for_type_known() {
        assert_eq!(ext_for_type(Some("flac")), "flac");
        assert_eq!(ext_for_type(Some("mp3")), "mp3");
        assert_eq!(ext_for_type(Some("FLAC")), "flac");
        assert_eq!(ext_for_type(None), "mp3");
    }
}
