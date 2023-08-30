// Copyright (c) 2023 Huawei Device Co., Ltd.
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use core::str;
use std::path::Path;

use crate::error::{ErrorKind, HttpError};

/// A type that defines the general structure of the `MIME` media typing system.
///
/// A `MIME` type most-commonly consists of just two parts:
///
/// - `Type`
/// - `Subtype`
///
/// `Type` and `SubType` are separated by a slash (/) — with no whitespace
/// between:
///
/// ```type/subtype```
/// 
/// It is case-insensitive but are traditionally written in lowercase, such as:
/// ```application/octet-stream```.
///
/// # Examples
///
/// ```
/// use ylong_http::body::MimeType;
///
/// let mime_type = MimeType::from_bytes(b"application/octet-stream").unwrap();
/// assert!(mime_type.is_application());
/// ```
#[derive(Debug, Eq, PartialEq)]
pub struct MimeType<'a> {
    tag: MimeTypeTag,
    bytes: &'a [u8],
    // Index of '/'.
    slash: usize,
}

impl<'a> MimeType<'a> {
    /// Creates a `MimeType` from a bytes slice.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::MimeType;
    ///
    /// let mime_type = MimeType::from_bytes(b"application/octet-stream").unwrap();
    /// assert_eq!(mime_type.main_type(), "application");
    /// assert_eq!(mime_type.sub_type(), "octet-stream");
    /// ```
    pub fn from_bytes(bytes: &'a [u8]) -> Result<Self, HttpError> {
        // From [RFC6838](http://tools.ietf.org/html/rfc6838#section-4.2):
        // <type-name> and <subtype-name> SHOULD be limited to 64 characters.
        // Both top-level type and subtype names are case-insensitive.

        let (slash, _) = bytes
            .iter()
            .enumerate()
            .find(|(_, &b)| b == b'/')
            .ok_or_else(|| HttpError::from(ErrorKind::InvalidInput))?;

        let tag = MimeTypeTag::from_bytes(&bytes[..slash])?;

        let sub_type = &bytes[slash + 1..];
        if sub_type.len() > 64 || !is_valid(sub_type) {
            return Err(ErrorKind::InvalidInput.into());
        }

        Ok(MimeType { tag, bytes, slash })
    }

    /// Creates a new `MimeType` from a file path. The extension of the file
    /// will be used to create it.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::path::Path;
    ///
    /// use ylong_http::body::MimeType;
    ///
    /// let path = Path::new("./foo/bar.pdf");
    /// let mime_type = MimeType::from_path(path).unwrap();
    /// assert_eq!(mime_type.main_type(), "application");
    /// assert_eq!(mime_type.sub_type(), "pdf");
    /// ```
    pub fn from_path(path: &'a Path) -> Result<Self, HttpError> {
        let str = path
            .extension()
            .and_then(|ext| ext.to_str())
            .ok_or_else(|| HttpError::from(ErrorKind::InvalidInput))?;
        Self::from_extension(str)
    }

    /// Returns a `&str` which represents the `MimeType`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::MimeType;
    ///
    /// let mime_type = MimeType::from_bytes(b"application/pdf").unwrap();
    /// assert_eq!(mime_type.as_str(), "application/pdf");
    /// ```
    pub fn as_str(&self) -> &str {
        // Safety: The input byte slice is checked, so it can be directly
        // converted to `&str` here.
        unsafe { str::from_utf8_unchecked(self.bytes) }
    }

    /// Returns main type string, such as `text` of `text/plain`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::MimeType;
    ///
    /// let mime_type = MimeType::from_bytes(b"application/pdf").unwrap();
    /// assert_eq!(mime_type.main_type(), "application");
    /// ```
    pub fn main_type(&self) -> &str {
        // Safety: The input byte slice is checked, so it can be directly
        // converted to `&str` here.
        unsafe { str::from_utf8_unchecked(&self.bytes[..self.slash]) }
    }

    /// Returns sub type string, such as `plain` of `text/plain`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::MimeType;
    ///
    /// let mime_type = MimeType::from_bytes(b"application/pdf").unwrap();
    /// assert_eq!(mime_type.sub_type(), "pdf");
    /// ```
    pub fn sub_type(&self) -> &str {
        // Safety: The input byte slice is checked, so it can be directly
        // converted to `&str` here.
        unsafe { str::from_utf8_unchecked(&self.bytes[self.slash + 1..]) }
    }

    /// Checks whether the main type is `application`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::MimeType;
    ///
    /// let mime_type = MimeType::from_bytes(b"application/pdf").unwrap();
    /// assert!(mime_type.is_application());
    /// ```
    pub fn is_application(&self) -> bool {
        matches!(self.tag, MimeTypeTag::Application)
    }

    /// Checks whether the main type is `audio`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::MimeType;
    ///
    /// let mime_type = MimeType::from_bytes(b"audio/basic").unwrap();
    /// assert!(mime_type.is_audio());
    /// ```
    pub fn is_audio(&self) -> bool {
        matches!(self.tag, MimeTypeTag::Audio)
    }

    /// Checks whether the main type is `font`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::MimeType;
    ///
    /// let mime_type = MimeType::from_bytes(b"font/collection").unwrap();
    /// assert!(mime_type.is_font());
    /// ```
    pub fn is_font(&self) -> bool {
        matches!(self.tag, MimeTypeTag::Font)
    }

    /// Checks whether the main type is `image`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::MimeType;
    ///
    /// let mime_type = MimeType::from_bytes(b"image/gif").unwrap();
    /// assert!(mime_type.is_image());
    /// ```
    pub fn is_image(&self) -> bool {
        matches!(self.tag, MimeTypeTag::Image)
    }

    /// Checks whether the main type is `message`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::MimeType;
    ///
    /// let mime_type = MimeType::from_bytes(b"message/rfc822").unwrap();
    /// assert!(mime_type.is_message());
    /// ```
    pub fn is_message(&self) -> bool {
        matches!(self.tag, MimeTypeTag::Message)
    }

    /// Checks whether the main type is `model`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::MimeType;
    ///
    /// let mime_type = MimeType::from_bytes(b"model/e57").unwrap();
    /// assert!(mime_type.is_model());
    /// ```
    pub fn is_model(&self) -> bool {
        matches!(self.tag, MimeTypeTag::Model)
    }

    /// Checks whether the main type is `multipart`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::MimeType;
    ///
    /// let mime_type = MimeType::from_bytes(b"multipart/form-data").unwrap();
    /// assert!(mime_type.is_multipart());
    /// ```
    pub fn is_multipart(&self) -> bool {
        matches!(self.tag, MimeTypeTag::Multipart)
    }

    /// Checks whether the main type is `text`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::MimeType;
    ///
    /// let mime_type = MimeType::from_bytes(b"text/richtext").unwrap();
    /// assert!(mime_type.is_text());
    /// ```
    pub fn is_text(&self) -> bool {
        matches!(self.tag, MimeTypeTag::Text)
    }

    /// Checks whether the main type is `video`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::MimeType;
    ///
    /// let mime_type = MimeType::from_bytes(b"video/mpeg").unwrap();
    /// assert!(mime_type.is_video());
    /// ```
    pub fn is_video(&self) -> bool {
        matches!(self.tag, MimeTypeTag::Video)
    }

    /// Checks whether the main type is non-standard type `x-`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::MimeType;
    ///
    /// let mime_type = MimeType::from_bytes(b"x-world/x-vrml").unwrap();
    /// assert!(mime_type.is_xnew());
    /// ```
    pub fn is_xnew(&self) -> bool {
        matches!(self.tag, MimeTypeTag::Xnew)
    }
}

impl<'a> Default for MimeType<'a> {
    fn default() -> Self {
        Self {
            tag: MimeTypeTag::Application,
            bytes: b"application/octet-stream",
            slash: 11,
        }
    }
}

macro_rules! mime {
    ($($ext: expr, $str: expr, $slash: expr, $tag: expr$(;)?)*) => {
        impl MimeType<'_> {
            /// Creates a new `MimeType` from file extension.
            ///
            /// Returns `application/octet-stream` if extension is not discerned.
            ///
            /// # Examples
            ///
            /// ```
            /// use ylong_http::body::MimeType;
            ///
            /// let mime_type = MimeType::from_extension("pdf").unwrap();
            /// assert_eq!(mime_type.main_type(), "application");
            /// assert_eq!(mime_type.sub_type(), "pdf");
            /// ```
            pub fn from_extension(s: &str) -> Result<Self, HttpError> {
                Ok(match s {
                    $(
                        $ext => MimeType {
                            tag: $tag,
                            bytes: $str.as_bytes(),
                            slash: $slash,
                        },
                    )*
                    _=> MimeType {
                        tag: MimeTypeTag::Application,
                        bytes: b"application/octet-stream",
                        slash: 11,
                    }
                })
            }
        }

        /// UT test cases for `ut_mime_type_from_extension`.
        ///
        /// # Brief
        /// 1. Creates a `MimeType` from file extension.
        /// 2. Checks if the test results are correct.
        #[test]
        fn ut_mime_type_from_extension() {
            $(
                let mime_type = MimeType::from_extension($ext).unwrap();
                assert_eq!(mime_type.tag, $tag);
                assert_eq!(mime_type.bytes, $str.as_bytes());
                assert_eq!(mime_type.slash, $slash);
            )*
        }
    };
}

mime!(
    "evy", "application/envoy", 11, MimeTypeTag::Application;
    "fif", "application/fractals", 11, MimeTypeTag::Application;
    "spl", "application/futuresplash", 11, MimeTypeTag::Application;
    "hta", "application/hta", 11, MimeTypeTag::Application;
    "acx", "application/internet-property-stream", 11, MimeTypeTag::Application;
    "hqx", "application/mac-binhex40", 11, MimeTypeTag::Application;
    "doc", "application/msword", 11, MimeTypeTag::Application;
    "dot", "application/msword", 11, MimeTypeTag::Application;
    "*", "application/octet-stream", 11, MimeTypeTag::Application;
    "bin", "application/octet-stream", 11, MimeTypeTag::Application;
    "class", "application/octet-stream", 11, MimeTypeTag::Application;
    "dms", "application/octet-stream", 11, MimeTypeTag::Application;
    "exe", "application/octet-stream", 11, MimeTypeTag::Application;
    "lha", "application/octet-stream", 11, MimeTypeTag::Application;
    "lzh", "application/octet-stream", 11, MimeTypeTag::Application;
    "oda", "application/oda", 11, MimeTypeTag::Application;
    "axs", "application/olescript", 11, MimeTypeTag::Application;
    "pdf", "application/pdf", 11, MimeTypeTag::Application;
    "prf", "application/pics-rules", 11, MimeTypeTag::Application;
    "p10", "application/pkcs10", 11, MimeTypeTag::Application;
    "crl", "application/pkix-crl", 11, MimeTypeTag::Application;
    "ai", "application/postscript", 11, MimeTypeTag::Application;
    "eps", "application/postscript", 11, MimeTypeTag::Application;
    "ps", "application/postscript", 11, MimeTypeTag::Application;
    "rtf", "application/rtf", 11, MimeTypeTag::Application;
    "setpay", "application/set-payment-initiation", 11, MimeTypeTag::Application;
    "setreg", "application/set-registration-initiation", 11, MimeTypeTag::Application;
    "xla", "application/vnd.ms-excel", 11, MimeTypeTag::Application;
    "xlc", "application/vnd.ms-excel", 11, MimeTypeTag::Application;
    "xlm", "application/vnd.ms-excel", 11, MimeTypeTag::Application;
    "xls", "application/vnd.ms-excel", 11, MimeTypeTag::Application;
    "xlt", "application/vnd.ms-excel", 11, MimeTypeTag::Application;
    "xlw", "application/vnd.ms-excel", 11, MimeTypeTag::Application;
    "msg", "application/vnd.ms-outlook", 11, MimeTypeTag::Application;
    "sst", "application/vnd.ms-pkicertstore", 11, MimeTypeTag::Application;
    "cat", "application/vnd.ms-pkiseccat", 11, MimeTypeTag::Application;
    "stl", "application/vnd.ms-pkistl", 11, MimeTypeTag::Application;
    "pot", "application/vnd.ms-powerpoint", 11, MimeTypeTag::Application;
    "pps", "application/vnd.ms-powerpoint", 11, MimeTypeTag::Application;
    "ppt", "application/vnd.ms-powerpoint", 11, MimeTypeTag::Application;
    "mpp", "application/vnd.ms-project", 11, MimeTypeTag::Application;
    "wcm", "application/vnd.ms-works", 11, MimeTypeTag::Application;
    "wdb", "application/vnd.ms-works", 11, MimeTypeTag::Application;
    "wks", "application/vnd.ms-works", 11, MimeTypeTag::Application;
    "wps", "application/vnd.ms-works", 11, MimeTypeTag::Application;
    "hlp", "application/winhlp", 11, MimeTypeTag::Application;
    "bcpio", "application/x-bcpio", 11, MimeTypeTag::Application;
    // "cdf" also can be "application/x-netcdf"
    "cdf", "application/x-cdf", 11, MimeTypeTag::Application;
    "z", "application/x-compress", 11, MimeTypeTag::Application;
    "tgz", "application/x-compressed", 11, MimeTypeTag::Application;
    "cpio", "application/x-cpio", 11, MimeTypeTag::Application;
    "csh", "application/x-csh", 11, MimeTypeTag::Application;
    "dcr", "application/x-director", 11, MimeTypeTag::Application;
    "dir", "application/x-director", 11, MimeTypeTag::Application;
    "dxr", "application/x-director", 11, MimeTypeTag::Application;
    "dvi", "application/x-dvi", 11, MimeTypeTag::Application;
    "gtar", "application/x-gtar", 11, MimeTypeTag::Application;
    "gz", "application/x-gzip", 11, MimeTypeTag::Application;
    "hdf", "application/x-hdf", 11, MimeTypeTag::Application;
    "ins", "application/x-internet-signup", 11, MimeTypeTag::Application;
    "isp", "application/x-internet-signup", 11, MimeTypeTag::Application;
    "iii", "application/x-iphone", 11, MimeTypeTag::Application;
    "js", "application/x-javascript", 11, MimeTypeTag::Application;
    "latex", "application/x-latex", 11, MimeTypeTag::Application;
    "mdb", "application/x-msaccess", 11, MimeTypeTag::Application;
    "crd", "application/x-mscardfile", 11, MimeTypeTag::Application;
    "clp", "application/x-msclip", 11, MimeTypeTag::Application;
    "dll", "application/x-msdownload", 11, MimeTypeTag::Application;
    "m13", "application/x-msmediaview", 11, MimeTypeTag::Application;
    "m14", "application/x-msmediaview", 11, MimeTypeTag::Application;
    "mvb", "application/x-msmediaview", 11, MimeTypeTag::Application;
    "wmf", "application/x-msmetafile", 11, MimeTypeTag::Application;
    "mny", "application/x-msmoney", 11, MimeTypeTag::Application;
    "pub", "application/x-mspublisher", 11, MimeTypeTag::Application;
    "scd", "application/x-msschedule", 11, MimeTypeTag::Application;
    "trm", "application/x-msterminal", 11, MimeTypeTag::Application;
    "wri", "application/x-mswrite", 11, MimeTypeTag::Application;
    "nc", "application/x-netcdf", 11, MimeTypeTag::Application;
    "pma", "application/x-perfmon", 11, MimeTypeTag::Application;
    "pmc", "application/x-perfmon", 11, MimeTypeTag::Application;
    "pml", "application/x-perfmon", 11, MimeTypeTag::Application;
    "pmr", "application/x-perfmon", 11, MimeTypeTag::Application;
    "pmw", "application/x-perfmon", 11, MimeTypeTag::Application;
    "p12", "application/x-pkcs12", 11, MimeTypeTag::Application;
    "pfx", "application/x-pkcs12", 11, MimeTypeTag::Application;
    "p7b", "application/x-pkcs7-certificates", 11, MimeTypeTag::Application;
    "spc", "application/x-pkcs7-certificates", 11, MimeTypeTag::Application;
    "p7r", "application/x-pkcs7-certificates", 11, MimeTypeTag::Application;
    "p7c", "application/x-pkcs7-mime", 11, MimeTypeTag::Application;
    "p7m", "application/x-pkcs7-mime", 11, MimeTypeTag::Application;
    "p7s", "application/x-pkcs7-signature", 11, MimeTypeTag::Application;
    "sh", "application/x-sh", 11, MimeTypeTag::Application;
    "shar", "application/x-shar", 11, MimeTypeTag::Application;
    "swf", "application/x-shockwave-flash", 11, MimeTypeTag::Application;
    "sit", "application/x-stuffit", 11, MimeTypeTag::Application;
    "sv4cpio", "application/x-sv4cpio", 11, MimeTypeTag::Application;
    "sv4crc", "application/x-sv4crc", 11, MimeTypeTag::Application;
    "tar", "application/x-tar", 11, MimeTypeTag::Application;
    "tcl", "application/x-tcl", 11, MimeTypeTag::Application;
    "tex", "application/x-tex", 11, MimeTypeTag::Application;
    "texi", "application/x-texinfo", 11, MimeTypeTag::Application;
    "texinfo", "application/x-texinfo", 11, MimeTypeTag::Application;
    "roff", "application/x-troff", 11, MimeTypeTag::Application;
    "t", "application/x-troff", 11, MimeTypeTag::Application;
    "tr", "application/x-troff", 11, MimeTypeTag::Application;
    "man", "application/x-troff-man", 11, MimeTypeTag::Application;
    "me", "application/x-troff-me", 11, MimeTypeTag::Application;
    "ms", "application/x-troff-ms", 11, MimeTypeTag::Application;
    "ustar", "application/x-ustar", 11, MimeTypeTag::Application;
    "src", "application/x-wais-source", 11, MimeTypeTag::Application;
    "cer", "application/x-x509-ca-cert", 11, MimeTypeTag::Application;
    "crt", "application/x-x509-ca-cert", 11, MimeTypeTag::Application;
    "der", "application/x-x509-ca-cert", 11, MimeTypeTag::Application;
    "pko", "application/ynd.ms-pkipko", 11, MimeTypeTag::Application;
    "zip", "application/zip", 11, MimeTypeTag::Application;
    "au", "audio/basic", 5, MimeTypeTag::Audio;
    "snd", "audio/basic", 5, MimeTypeTag::Audio;
    "mid", "audio/mid", 5, MimeTypeTag::Audio;
    "rmi", "audio/mid", 5, MimeTypeTag::Audio;
    "mp3", "audio/mpeg", 5, MimeTypeTag::Audio;
    "aif", "audio/x-aiff", 5, MimeTypeTag::Audio;
    "aifc", "audio/x-aiff", 5, MimeTypeTag::Audio;
    "aiff", "audio/x-aiff", 5, MimeTypeTag::Audio;
    "m3u", "audio/x-mpegurl", 5, MimeTypeTag::Audio;
    "ra", "audio/x-pn-realaudio", 5, MimeTypeTag::Audio;
    "ram", "audio/x-pn-realaudio", 5, MimeTypeTag::Audio;
    "wav", "audio/x-wav", 5, MimeTypeTag::Audio;
    "bmp", "image/bmp", 5, MimeTypeTag::Image;
    "cod", "image/cis-cod", 5, MimeTypeTag::Image;
    "gif", "image/gif", 5, MimeTypeTag::Image;
    "ief", "image/ief", 5, MimeTypeTag::Image;
    "jpe", "image/jpeg", 5, MimeTypeTag::Image;
    "jpeg", "image/jpeg", 5, MimeTypeTag::Image;
    "jpg", "image/jpeg", 5, MimeTypeTag::Image;
    "jfif", "image/pipeg", 5, MimeTypeTag::Image;
    "svg", "image/svg+xml", 5, MimeTypeTag::Image;
    "tif", "image/tiff", 5, MimeTypeTag::Image;
    "tiff", "image/tiff", 5, MimeTypeTag::Image;
    "ras", "image/x-cmu-raster", 5, MimeTypeTag::Image;
    "cmx", "image/x-cmx", 5, MimeTypeTag::Image;
    "ico", "image/x-icon", 5, MimeTypeTag::Image;
    "pnm", "image/x-portable-anymap", 5, MimeTypeTag::Image;
    "pbm", "image/x-portable-bitmap", 5, MimeTypeTag::Image;
    "pgm", "image/x-portable-graymap", 5, MimeTypeTag::Image;
    "ppm", "image/x-portable-pixmap", 5, MimeTypeTag::Image;
    "rgb", "image/x-rgb", 5, MimeTypeTag::Image;
    "xbm", "image/x-xbitmap", 5, MimeTypeTag::Image;
    "xpm", "image/x-xpixmap", 5, MimeTypeTag::Image;
    "xwd", "image/x-xwindowdump", 5, MimeTypeTag::Image;
    "mht", "message/rfc822", 7, MimeTypeTag::Message;
    "mhtml", "message/rfc822", 7, MimeTypeTag::Message;
    "nws", "message/rfc822", 7, MimeTypeTag::Message;
    "css", "text/css", 4, MimeTypeTag::Text;
    "323", "text/h323", 4, MimeTypeTag::Text;
    "htm", "text/html", 4, MimeTypeTag::Text;
    "html", "text/html", 4, MimeTypeTag::Text;
    "stm", "text/html", 4, MimeTypeTag::Text;
    "uls", "text/iuls", 4, MimeTypeTag::Text;
    "bas", "text/plain", 4, MimeTypeTag::Text;
    "c", "text/plain", 4, MimeTypeTag::Text;
    "h", "text/plain", 4, MimeTypeTag::Text;
    "txt", "text/plain", 4, MimeTypeTag::Text;
    "rtx", "text/richtext", 4, MimeTypeTag::Text;
    "sct", "text/scriptlet", 4, MimeTypeTag::Text;
    "tsv", "text/tab-separated-values", 4, MimeTypeTag::Text;
    "htt", "text/webviewhtml", 4, MimeTypeTag::Text;
    "htc", "text/x-component", 4, MimeTypeTag::Text;
    "etx", "text/x-setext", 4, MimeTypeTag::Text;
    "vcf", "text/x-vcard", 4, MimeTypeTag::Text;
    "mp2", "video/mpeg", 5, MimeTypeTag::Video;
    "mpa", "video/mpeg", 5, MimeTypeTag::Video;
    "mpe", "video/mpeg", 5, MimeTypeTag::Video;
    "mpeg", "video/mpeg", 5, MimeTypeTag::Video;
    "mpg", "video/mpeg", 5, MimeTypeTag::Video;
    "mpv2", "video/mpeg", 5, MimeTypeTag::Video;
    "mov", "video/quicktime", 5, MimeTypeTag::Video;
    "qt", "video/quicktime", 5, MimeTypeTag::Video;
    "lsf", "video/x-la-asf", 5, MimeTypeTag::Video;
    "lsx", "video/x-la-asf", 5, MimeTypeTag::Video;
    "asf", "video/x-ms-asf", 5, MimeTypeTag::Video;
    "asr", "video/x-ms-asf", 5, MimeTypeTag::Video;
    "asx", "video/x-ms-asf", 5, MimeTypeTag::Video;
    "avi", "video/x-msvideo", 5, MimeTypeTag::Video;
    "movie", "video/x-sgi-movie", 5, MimeTypeTag::Video;
    "flr", "x-world/x-vrml", 7, MimeTypeTag::Xnew;
    "vrml", "x-world/x-vrml", 7, MimeTypeTag::Xnew;
    "wrl", "x-world/x-vrml", 7, MimeTypeTag::Xnew;
    "wrz", "x-world/x-vrml", 7, MimeTypeTag::Xnew;
    "xaf", "x-world/x-vrml", 7, MimeTypeTag::Xnew;
    "xof", "x-world/x-vrml", 7, MimeTypeTag::Xnew;
);

/// `MIME` main type.
#[derive(Debug, PartialEq, Eq)]
enum MimeTypeTag {
    Application,
    Audio,
    Font,
    Image,
    Message,
    Model,
    Multipart,
    Text,
    Video,
    // A type not included in the standard, beginning with `x-`
    Xnew,
}

impl MimeTypeTag {
    fn from_bytes(b: &[u8]) -> Result<Self, HttpError> {
        // From [RFC6838](http://tools.ietf.org/html/rfc6838#section-4.2)
        // <type-name> and <subtype-name> SHOULD be limited to 64 characters.
        // Both top-level type and subtype names are case-insensitive.

        if b.len() > 64 || b.len() < 2 {
            return Err(ErrorKind::InvalidInput.into());
        }

        match b[0].to_ascii_lowercase() {
            // beginning with "x-"
            b'x' => {
                if b[1] == b'-' && is_valid(&b[2..]) {
                    return Ok(Self::Xnew);
                }
            }
            b'a' => {
                // application
                if b[1..].eq_ignore_ascii_case(b"pplication") {
                    return Ok(Self::Application);
                // audio
                } else if b[1..].eq_ignore_ascii_case(b"udio") {
                    return Ok(Self::Audio);
                }
            }
            b'f' => {
                // font
                if b[1..].eq_ignore_ascii_case(b"ont") {
                    return Ok(Self::Font);
                }
            }
            b'i' => {
                // image
                if b[1..].eq_ignore_ascii_case(b"mage") {
                    return Ok(Self::Image);
                }
            }
            b'm' => {
                // message
                if b[1..].eq_ignore_ascii_case(b"essage") {
                    return Ok(Self::Message);
                // model
                } else if b[1..].eq_ignore_ascii_case(b"odel") {
                    return Ok(Self::Model);
                // multipart
                } else if b[1..].eq_ignore_ascii_case(b"ultipart") {
                    return Ok(Self::Multipart);
                }
            }
            b't' => {
                // text
                if b[1..].eq_ignore_ascii_case(b"ext") {
                    return Ok(Self::Text);
                }
            }
            b'v' => {
                // video
                if b[1..].eq_ignore_ascii_case(b"ideo") {
                    return Ok(Self::Video);
                }
            }
            _ => return Err(ErrorKind::InvalidInput.into()),
        };

        Err(ErrorKind::InvalidInput.into())
    }
}

// From [RFC6838](http://tools.ietf.org/html/rfc6838#section-4.2):
//
// All registered media types MUST be assigned top-level type and
// subtype names.  The combination of these names serves to uniquely
// identify the media type, and the subtype name facet (or the absence
// of one) identifies the registration tree.  Both top-level type and
// subtype names are case-insensitive.
//
// Type and subtype names MUST conform to the following ABNF:
//
//     type-name = restricted-name
//     subtype-name = restricted-name
//
//     restricted-name = restricted-name-first *126restricted-name-chars
//     restricted-name-first  = ALPHA / DIGIT
//     restricted-name-chars  = ALPHA / DIGIT / "!" / "#" /
//                              "$" / "&" / "-" / "^" / "_"
//     restricted-name-chars =/ "." ; Characters before first dot always
//                                  ; specify a facet name
//     restricted-name-chars =/ "+" ; Characters after last plus always
//                                  ; specify a structured syntax suffix
#[rustfmt::skip]
static MEDIA_TYPE_VALID_BYTES: [bool; 256] = {
    const __: bool = false;
    const TT: bool = true;
    [
//      \0                                  HT  LF          CR
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __,
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __,
//      \w  !   "   #   $   %   &   '   (   )   *   +   ,   -   .   /
        __, TT, __, TT, TT, __, TT, __, __, __, __, TT, __, TT, TT, __,
//       0   1   2   3   4   5   6   7   8   9   :   ;   <   =   >   ?
        TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, __, __, __, __, __, __,
//       @   A   B   C   D   E   F   G   H   I   J   K   L   M   N   O
        __, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT,
//       P   Q   R   S   T   U   V   W   X   Y   Z   [   \   ]   ^   _
        TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, __, __, __, TT, TT,
//       `   a   b   c   d   e   f   g   h   i   j   k   l   m   n   o
        __, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT,
//       p   q   r   s   t   u   v   w   x   y   z   {   |   }   ~   del
        TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, __, __, __, __, __,
// Expand ascii
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __,
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __,
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __,
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __,
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __,
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __,
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __,
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __,
    ]
};

fn is_valid(v: &[u8]) -> bool {
    v.iter().all(|b| MEDIA_TYPE_VALID_BYTES[*b as usize])
}

#[cfg(test)]
mod ut_mime {
    use super::{MimeType, MimeTypeTag};
    use crate::error::{ErrorKind, HttpError};

    /// UT test cases for `MimeTypeTag::from_bytes`.
    ///
    /// # Brief
    /// 1. Creates a `MimeTypeTag` from `&[u8]`.
    /// 2. Checks if the test results are correct.
    #[test]
    fn ut_mime_type_tag_from_bytes() {
        assert_eq!(
            MimeTypeTag::from_bytes(b"application"),
            Ok(MimeTypeTag::Application)
        );
        assert_eq!(MimeTypeTag::from_bytes(b"audio"), Ok(MimeTypeTag::Audio));
        assert_eq!(MimeTypeTag::from_bytes(b"font"), Ok(MimeTypeTag::Font));
        assert_eq!(MimeTypeTag::from_bytes(b"image"), Ok(MimeTypeTag::Image));
        assert_eq!(
            MimeTypeTag::from_bytes(b"message"),
            Ok(MimeTypeTag::Message)
        );
        assert_eq!(MimeTypeTag::from_bytes(b"model"), Ok(MimeTypeTag::Model));
        assert_eq!(
            MimeTypeTag::from_bytes(b"multipart"),
            Ok(MimeTypeTag::Multipart)
        );
        assert_eq!(MimeTypeTag::from_bytes(b"text"), Ok(MimeTypeTag::Text));
        assert_eq!(MimeTypeTag::from_bytes(b"video"), Ok(MimeTypeTag::Video));
        assert_eq!(MimeTypeTag::from_bytes(b"x-world"), Ok(MimeTypeTag::Xnew));
        assert_eq!(
            MimeTypeTag::from_bytes(b"APPLICATION"),
            Ok(MimeTypeTag::Application)
        );
        assert_eq!(
            MimeTypeTag::from_bytes(b"x-ab/cd"),
            Err(HttpError::from(ErrorKind::InvalidInput))
        );
        assert_eq!(
            MimeTypeTag::from_bytes(b"notype"),
            Err(HttpError::from(ErrorKind::InvalidInput))
        );
    }

    /// UT test cases for `MimeType::from_bytes`.
    ///
    /// # Brief
    /// 1. Creates a `MimeType` from `&[u8]`.
    /// 2. Checks if the test results are correct.
    #[test]
    fn ut_mime_type_from_bytes() {
        assert_eq!(
            MimeType::from_bytes(b"application/octet-stream"),
            Ok(MimeType {
                tag: MimeTypeTag::Application,
                bytes: b"application/octet-stream",
                slash: 11,
            })
        );

        assert_eq!(
            MimeType::from_bytes(b"TEXT/PLAIN"),
            Ok(MimeType {
                tag: MimeTypeTag::Text,
                bytes: b"TEXT/PLAIN",
                slash: 4,
            })
        );

        assert_eq!(
            MimeType::from_bytes(b"TEXT/~PLAIN"),
            Err(HttpError::from(ErrorKind::InvalidInput))
        );

        assert_eq!(
            MimeType::from_bytes(b"application/octet/stream"),
            Err(HttpError::from(ErrorKind::InvalidInput))
        );
    }

    /// UT test cases for `MimeType::from_path`.
    ///
    /// # Brief
    /// 1. Creates a `MimeType` from path.
    /// 2. Checks if the test results are correct.
    #[test]
    fn ut_mime_type_from_path() {
        use std::path::Path;

        use crate::error::HttpError;

        assert_eq!(
            MimeType::from_path(Path::new("./foo/bar.evy")),
            MimeType::from_bytes(b"application/envoy")
        );
        assert_eq!(
            MimeType::from_path(Path::new("foo.*")),
            MimeType::from_bytes(b"application/octet-stream")
        );
        assert_eq!(
            MimeType::from_path(Path::new("")),
            Err(HttpError::from(ErrorKind::InvalidInput))
        );
        assert_eq!(
            MimeType::from_path(Path::new(".txt")),
            Err(HttpError::from(ErrorKind::InvalidInput))
        );
        assert_eq!(
            MimeType::from_path(Path::new("./foo/bar")),
            Err(HttpError::from(ErrorKind::InvalidInput))
        );
    }

    /// UT test cases for `MimeType::main_type`.
    ///
    /// # Brief
    /// 1. Gets main type string by `main_type`.
    /// 2. Checks if the test results are correct.
    #[test]
    fn ut_mime_type_main_type() {
        let mime_type = MimeType::from_bytes(b"application/octet-stream").unwrap();
        assert_eq!(mime_type.main_type(), "application");

        let mime_type = MimeType::from_bytes(b"TeXT/PLAIN").unwrap();
        assert_eq!(mime_type.main_type(), "TeXT");
    }

    /// UT test cases for `MimeType::sub_type`.
    ///
    /// # Brief
    /// 1. Gets subtype type string by `sub_type`.
    /// 2. Checks if the test results are correct.
    #[test]
    fn ut_mimetype_sub_type() {
        let mime_type = MimeType::from_bytes(b"application/octet-stream").unwrap();
        assert_eq!(mime_type.sub_type(), "octet-stream");
    }

    /// UT test cases for `MimeType::as_str`.
    ///
    /// # Brief
    /// 1. Gets string from `MimeType` by `as_str`.
    /// 2. Checks if the test results are correct.
    #[test]
    fn ut_mimetype_as_str() {
        let mime_type = MimeType::from_bytes(b"application/pdf").unwrap();
        assert_eq!(mime_type.as_str(), "application/pdf");

        let mime_type = MimeType::from_bytes(b"application/octet-stream").unwrap();
        assert_eq!(mime_type.as_str(), "application/octet-stream");
    }

    /// UT test cases for `Mimetype::eq`.
    ///
    /// # Brief
    /// 1. Creates some `MimeType`, and check if they are equal.
    /// 2. Checks if the test results are correct.
    #[test]
    fn ut_mime_type_eq() {
        assert_eq!(
            MimeType {
                tag: MimeTypeTag::Application,
                bytes: b"application/octet-stream",
                slash: 11,
            },
            MimeType {
                tag: MimeTypeTag::Application,
                bytes: b"application/octet-stream",
                slash: 11,
            }
        );

        assert_ne!(
            MimeType::from_bytes(b"application/octet-stream"),
            MimeType::from_bytes(b"application/pdf")
        );

        assert_eq!(
            MimeType::from_extension("pdf"),
            MimeType::from_bytes(b"application/pdf")
        );
    }

    /// UT test cases for `MimeType::is_application`.
    ///
    /// # Brief
    /// 1. Creates `MimeType` from `&[u8]` by `MimeType::from_bytes`.
    /// 2. Checks whether the main types are correct.
    #[test]
    fn ut_mimetype_is_application() {
        assert!(MimeType::from_bytes(b"application/pdf")
            .unwrap()
            .is_application());
        assert!(!MimeType::from_bytes(b"audio/basic")
            .unwrap()
            .is_application());
    }

    /// UT test cases for `MimeType::is_audio`.
    ///
    /// # Brief
    /// 1. Creates `MimeType` from `&[u8]` by `MimeType::from_bytes`.
    /// 2. Checks whether the main types are correct.
    #[test]
    fn ut_mimetype_is_audio() {
        assert!(MimeType::from_bytes(b"audio/basic").unwrap().is_audio());
        assert!(!MimeType::from_bytes(b"application/pdf").unwrap().is_audio());
    }

    /// UT test cases for `MimeType::is_font`.
    ///
    /// # Brief
    /// 1. Creates `MimeType` from `&[u8]` by `MimeType::from_bytes`.
    /// 2. Checks whether the main types are correct.
    #[test]
    fn ut_mime_type_is_font() {
        assert!(MimeType::from_bytes(b"font/collection").unwrap().is_font());
        assert!(!MimeType::from_bytes(b"application/pdf").unwrap().is_font());
    }

    /// UT test cases for `MimeType::is_image`.
    ///
    /// # Brief
    /// 1. Creates `MimeType` from `&[u8]` by `MimeType::from_bytes`.
    /// 2. Checks whether the main types are correct.
    #[test]
    fn ut_mimetype_is_image() {
        assert!(MimeType::from_bytes(b"image/bmp").unwrap().is_image());
        assert!(!MimeType::from_bytes(b"application/pdf").unwrap().is_image());
    }

    /// UT test cases for `MimeType::is_message`.
    ///
    /// # Brief
    /// 1. Creates `MimeType` from `&[u8]` by `MimeType::from_bytes`.
    /// 2. Checks whether the main types are correct.
    #[test]
    fn ut_mimetype_is_message() {
        assert!(MimeType::from_bytes(b"message/example")
            .unwrap()
            .is_message());
        assert!(!MimeType::from_bytes(b"application/pdf")
            .unwrap()
            .is_message());
    }

    /// UT test cases for `MimeType::is_model`.
    ///
    /// # Brief
    /// 1. Creates `MimeType` from `&[u8]` by `MimeType::from_bytes`.
    /// 2. Checks whether the main types are correct.
    #[test]
    fn ut_mimetype_is_model() {
        assert!(MimeType::from_bytes(b"model/e57").unwrap().is_model());
        assert!(!MimeType::from_bytes(b"application/pdf").unwrap().is_model());
    }

    /// UT test cases for `MimeType::is_multipart`.
    ///
    /// # Brief
    /// 1. Creates `MimeType` from `&[u8]` by `MimeType::from_bytes`.
    /// 2. Checks whether the main types are correct.
    #[test]
    fn ut_mime_type_is_multipart() {
        assert!(MimeType::from_bytes(b"multipart/form-data")
            .unwrap()
            .is_multipart());
        assert!(!MimeType::from_bytes(b"application/pdf")
            .unwrap()
            .is_multipart());
    }

    /// UT test cases for `MimeType::is_text`.
    ///
    /// # Brief
    /// 1. Creates `MimeType` from `&[u8]` by `MimeType::from_bytes`.
    /// 2. Checks whether the main types are correct.
    #[test]
    fn ut_mime_type_is_text() {
        assert!(MimeType::from_bytes(b"text/csv").unwrap().is_text());
        assert!(!MimeType::from_bytes(b"application/pdf").unwrap().is_text());
    }

    /// UT test cases for `MimeType::is_video`.
    ///
    /// # Brief
    /// 1. Creates `MimeType` from `&[u8]` by `MimeType::from_bytes`.
    /// 2. Checks whether the main types are correct.
    #[test]
    fn ut_mimetype_is_video() {
        assert!(MimeType::from_bytes(b"video/mpeg").unwrap().is_video());
        assert!(!MimeType::from_bytes(b"application/pdf").unwrap().is_video());
    }

    /// UT test cases for `MimeType::is_xnew`.
    ///
    /// # Brief
    /// 1. Creates `MimeType` from `&[u8]` by `MimeType::from_bytes`.
    /// 2. Checks whether the main types are correct.
    #[test]
    fn ut_mime_type_is_xnew() {
        assert!(MimeType::from_bytes(b"x-world/x-vrml").unwrap().is_xnew());
        assert!(!MimeType::from_bytes(b"application/pdf").unwrap().is_xnew());
    }
}
