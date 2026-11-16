// Dữ liệu giả sát ngữ cảnh VietKaraStudio (không kết nối backend)
window.MOCK_DATA = {
  projects: [
    {
      id: "ha-trang",
      name: "Hạ Trắng – Trịnh Công Sơn",
      artist: "Quang Dũng & Lệ Quyên",
      saved: true,
      duration: 42.0, // bản cắt mẫu 42s cho timeline dễ thử
      hasReference: true,
      referenceFile: "HaTrang_Vocal_Master.wav",
      hasBeat: true,
      beatFile: "HaTrang_BeatGoc_24bit.wav",
      hasBacking: true,
      backingAccepted: true,
      singingMode: "duet",
      style: {
        font: "Segoe UI",
        fontSize: 34,
        align: "center",
        sungColor: "#e8b04a",
        unsungColor: "#ffffff",
        outlineWidth: 2,
        shadow: 4,
        lineGap: 10,
        aspect: "landscape",
        resolution: "1080p",
        bgType: "gradient",
        bgColor: "#141b2d",
        showTitle: true,
        titleText: "HẠ TRẮNG",
        titleArtist: "Trịnh Công Sơn",
        showLogo: true,
        logoText: "VIETKARA",
        countdown: "mic"
      },
      lines: [
        { id: 1, role: "all", locked: true, start: 2.0, words: [
          { t: "Gọi", s: 2.0, e: 2.6 }, { t: "nắng", s: 2.6, e: 3.3 }, { t: "trên", s: 3.3, e: 3.8 },
          { t: "vai", s: 3.8, e: 4.3 }, { t: "em", s: 4.3, e: 4.8 }, { t: "gầy", s: 4.8, e: 5.7 }
        ]},
        { id: 2, role: "female", locked: true, start: 6.1, words: [
          { t: "đường", s: 6.1, e: 6.7 }, { t: "xa", s: 6.7, e: 7.3 }, { t: "áo", s: 7.3, e: 7.9 }, { t: "bay", s: 7.9, e: 9.1 }
        ]},
        { id: 3, role: "female", locked: true, start: 9.6, words: [
          { t: "Nắng", s: 9.6, e: 10.3 }, { t: "qua", s: 10.3, e: 10.8 }, { t: "mắt", s: 10.8, e: 11.4 }, { t: "buồn", s: 11.4, e: 12.6 }
        ]},
        { id: 4, role: "male", locked: false, start: 13.1, words: [
          { t: "lòng", s: 13.1, e: 13.7 }, { t: "hoa", s: 13.7, e: 14.3 }, { t: "bướm", s: 14.3, e: 14.9 }, { t: "say", s: 14.9, e: 16.0 }
        ]},
        { id: 5, role: "male", locked: false, start: 16.6, words: [
          { t: "Lạy", s: 16.6, e: 17.2 }, { t: "trời", s: 17.2, e: 17.8 }, { t: "mưa", s: 17.8, e: 18.4 }, { t: "phai", s: 18.4, e: 19.6 }
        ]},
        { id: 6, role: "all", locked: false, start: 20.2, words: [
          { t: "Hạ", s: 20.2, e: 20.9 }, { t: "trắng", s: 20.9, e: 21.8 }, { t: "ơi", s: 21.8, e: 23.0 }
        ]},
        { id: 7, role: "female", locked: false, start: 23.5, words: [
          { t: "Gọi", s: 23.5, e: 24.1 }, { t: "tình", s: 24.1, e: 24.7 }, { t: "xanh", s: 24.7, e: 25.3 }, { t: "xao", s: 25.3, e: 26.5 }
        ]},
        { id: 8, role: "male", locked: false, start: 27.0, words: [
          { t: "dưới", s: 27.0, e: 27.6 }, { t: "gót", s: 27.6, e: 28.2 }, { t: "chân", s: 28.2, e: 28.8 }, { t: "em", s: 28.8, e: 30.1 }
        ]}
      ]
    },
    {
      id: "diem-xua",
      name: "Diễm Xưa – Trịnh Công Sơn",
      artist: "Khánh Ly",
      saved: true,
      duration: 30.0,
      hasReference: true,
      referenceFile: "DiemXua_Master.mp3",
      hasBeat: true,
      beatFile: "DiemXua_Beat.wav",
      hasBacking: false,
      backingAccepted: false,
      singingMode: "solo",
      style: {
        font: "Times New Roman", fontSize: 34, align: "center",
        sungColor: "#4aa3e8", unsungColor: "#ffffff", outlineWidth: 2, shadow: 4, lineGap: 10,
        aspect: "landscape", resolution: "1080p", bgType: "color", bgColor: "#111824",
        showTitle: true, titleText: "DIỄM XƯA", titleArtist: "Trịnh Công Sơn",
        showLogo: false, logoText: "VIETKARA", countdown: "star"
      },
      lines: [
        { id: 1, role: "all", locked: true, start: 2.0, words: [
          { t: "Mưa", s: 2.0, e: 2.7 }, { t: "vẫn", s: 2.7, e: 3.3 }, { t: "mưa", s: 3.3, e: 3.9 },
          { t: "bay", s: 3.9, e: 4.6 }, { t: "trên", s: 4.6, e: 5.1 }, { t: "tầng", s: 5.1, e: 5.8 },
          { t: "tháp", s: 5.8, e: 6.4 }, { t: "cổ", s: 6.4, e: 7.4 }
        ]},
        { id: 2, role: "all", locked: false, start: 8.0, words: [
          { t: "Dài", s: 8.0, e: 8.7 }, { t: "tay", s: 8.7, e: 9.3 }, { t: "em", s: 9.3, e: 9.9 },
          { t: "mấy", s: 9.9, e: 10.6 }, { t: "thuở", s: 10.6, e: 11.4 }, { t: "mắt", s: 11.4, e: 12.1 },
          { t: "xanh", s: 12.1, e: 12.8 }, { t: "xao", s: 12.8, e: 13.9 }
        ]}
      ]
    }
  ],
  presets: [
    { id: "01_default", name: "Mặc định VietKara", sung: "#e8b04a", unsung: "#ffffff", font: "Segoe UI", desc: "Vàng ấm trên nền tối, dễ đọc" },
    { id: "03_gaming_neon", name: "Neon Gaming", sung: "#39e5ff", unsung: "#f4f6ff", font: "Segoe UI", desc: "Xanh neon tương phản cao" },
    { id: "06_vang_champagne", name: "Vàng Champagne", sung: "#f4d078", unsung: "#fff7e8", font: "Tahoma", desc: "Nhạc trữ tình, bolero" },
    { id: "07_hong_ngoc_rose", name: "Hồng Ngọc Rose", sung: "#ff72a6", unsung: "#ffffff", font: "Verdana", desc: "Pop ballad, song ca" },
    { id: "13_dien_anh_serif", name: "Điện Ảnh Serif", sung: "#e8b04a", unsung: "#eae6dc", font: "Times New Roman", desc: "Phong cách phụ đề điện ảnh" },
    { id: "16_chrome_bac", name: "Chrome Bạc", sung: "#8ad4ff", unsung: "#e5e9f0", font: "Arial", desc: "Tối giản, hiện đại" }
  ],
  models: [
    { id: "whisper-small", group: "Nhận lời (Whisper CTranslate2)", name: "Whisper small", size: "488 MB", path: "data/models/whisper/small", status: "ok", note: "Nhanh, nhẹ" },
    { id: "whisper-large-v3-turbo", group: "Nhận lời (Whisper CTranslate2)", name: "Whisper large-v3-turbo", size: "1.62 GB", path: "data/models/whisper/large-v3-turbo", status: "ok", note: "Đang chọn · tiếng Việt chính xác" },
    { id: "demucs-htdemucs", group: "Tách nhạc (Demucs)", name: "htdemucs_ft", size: "320 MB", path: "data/models/htdemucs_ft.th", status: "ok", note: "Tách vocal / beat tiêu chuẩn" },
    { id: "mel-roformer", group: "Tách giọng bè", name: "Mel-RoFormer Karaoke", size: "913 MB", path: "data/models/backing/mel_roformer.ckpt", status: "missing", note: "Chất lượng cao · chưa tải" },
    { id: "uvr-karaoke", group: "Tách giọng bè", name: "UVR Karaoke (nhẹ)", size: "184 MB", path: "data/models/backing/uvr_karaoke.pth", status: "error", note: "Lỗi kiểm tra SHA-256 · cần tải lại" }
  ]
};
