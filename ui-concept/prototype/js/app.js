(() => {
  const $ = (s, root = document) => root.querySelector(s);
  const $$ = (s, root = document) => [...root.querySelectorAll(s)];
  const ico = (id, cls = "") => `<svg class="ico ${cls}"><use href="#${id}"/></svg>`;

  const ROLE_COLOR = {
    all: "var(--c-role-all)",
    female: "var(--c-role-female)",
    male: "var(--c-role-male)",
    unknown: "var(--c-role-unknown)"
  };

  const INSP_TITLES = {
    selection: "Từ đang chọn",
    style: "Kiểu lời",
    duet: "Song ca & vai",
    title: "Tiêu đề bài hát",
    bg: "Nền & tỉ lệ",
    logo: "Logo & Đếm ngược",
    preset: "Thư viện Preset"
  };

  const state = {
    page: "karaoke",
    demoMode: "ready", // ready | empty | busy | error
    project: structuredClone(window.MOCK_DATA.projects[0]),
    currentTime: 2.6,
    playing: false,
    playTimer: null,
    speed: 1,
    backingOn: true,
    selectedLineId: 1,
    selectedWordIdx: 1,
    inspTab: "selection",
    zoom: 100,
    showWaves: true,
    followPlayhead: true,
    linkEdges: true,
    tapping: false,
    tapIndex: 0,
    jobTimer: null
  };

  const fmtTime = (sec) => {
    const m = Math.floor(sec / 60);
    const s = (sec % 60).toFixed(2).padStart(5, "0");
    return `${String(m).padStart(2, "0")}:${s}`;
  };

  // ---------- 1. MENU BAR & DEMO STATES ----------
  $$(".menu-trigger").forEach(btn => {
    btn.addEventListener("click", (e) => {
      e.stopPropagation();
      const id = "menu-" + btn.dataset.menu;
      const target = document.getElementById(id);
      const wasOpen = !target.classList.contains("hidden");
      closeMenus();
      if (!wasOpen) {
        target.classList.remove("hidden");
        btn.classList.add("open");
      }
    });
  });

  function closeMenus() {
    $$(".menu-dropdown").forEach(d => d.classList.add("hidden"));
    $$(".menu-trigger").forEach(b => b.classList.remove("open"));
  }
  document.addEventListener("click", closeMenus);

  $$(".menu-item").forEach(item => {
    item.addEventListener("click", () => {
      closeMenus();
      runCommand(item.dataset.cmd);
    });
  });

  $$(".demo-pill").forEach(pill => {
    pill.addEventListener("click", () => setDemoMode(pill.dataset.demo));
  });

  function setDemoMode(mode) {
    state.demoMode = mode;
    $$(".demo-pill").forEach(p => p.classList.toggle("on", p.dataset.demo === mode));
    $("#globalErrorBanner").classList.toggle("hidden", mode !== "error");

    if (mode === "empty") {
      stopPlayback();
      state.project = {
        id: "empty",
        name: "Bài karaoke mới",
        saved: false,
        duration: 0,
        hasReference: false,
        referenceFile: "",
        hasBeat: false,
        beatFile: "",
        hasBacking: false,
        backingAccepted: false,
        singingMode: "solo",
        style: structuredClone(window.MOCK_DATA.projects[0].style),
        lines: []
      };
      state.currentTime = 0;
      state.selectedLineId = null;
      state.selectedWordIdx = null;
      setJobStatus("Sẵn sàng · Chưa mở dữ liệu", 0, false);
    } else if (mode === "ready") {
      if (!state.project.lines.length) {
        state.project = structuredClone(window.MOCK_DATA.projects[0]);
        state.currentTime = 2.6;
        state.selectedLineId = 1;
        state.selectedWordIdx = 1;
      }
      setJobStatus("Sẵn sàng", 0, false);
    } else if (mode === "busy") {
      if (!state.project.lines.length) state.project = structuredClone(window.MOCK_DATA.projects[0]);
      simulateJob("Đang nhận lời & căn từ (Whisper large-v3-turbo)…", 2400);
    } else if (mode === "error") {
      if (!state.project.lines.length) state.project = structuredClone(window.MOCK_DATA.projects[0]);
      setJobStatus("Tác vụ gặp lỗi · Kiểm tra đường dẫn hoặc model", 100, true, "err");
    }
    renderAll();
  }

  function runCommand(cmd) {
    switch (cmd) {
      case "open-projects": openProjectsDialog(); break;
      case "new-project": setDemoMode("empty"); break;
      case "load-sample":
        state.project = structuredClone(window.MOCK_DATA.projects[0]);
        state.currentTime = 2.6;
        state.selectedLineId = 1;
        state.selectedWordIdx = 1;
        setDemoMode("ready");
        break;
      case "open-export": $("#dlgExport").showModal(); break;
      case "open-ai-settings": $("#dlgAi").showModal(); break;
      case "open-sync-anchors": $("#dlgAnchors").showModal(); break;
      case "show-shortcuts": $("#dlgShortcuts").showModal(); break;
      case "edit-lyrics":
        switchLeftTab("lyrics");
        $("#lyricsEditBox").classList.remove("hidden");
        $("#lyricsTextarea").value = state.project.lines.map(l => l.words.map(w => w.t).join(" ")).join("\n");
        break;
      case "auto-align":
        simulateJob("Mô phỏng: Đang nhận lời & căn từ theo beat…", 1800, () => {
          setJobStatus("Đã căn xong 8 câu (mô phỏng)", 100, true, "ok");
        });
        break;
      case "toggle-tap": toggleTapping(); break;
      case "go-separation": switchPage("separation"); break;
      case "toggle-left": toggleLeftPanel(); break;
      case "toggle-right": toggleRightPanel(); break;
      case "toggle-waves":
        state.showWaves = !state.showWaves;
        $("#btnToggleWaves").classList.toggle("on", state.showWaves);
        renderTimeline();
        break;
      case "reset-layout":
        $("#studioGrid").classList.remove("left-collapsed", "right-collapsed");
        $("#leftPanel").classList.remove("is-collapsed");
        $("#rightPanel").classList.remove("is-collapsed");
        $("#btnCollapseLeft").innerHTML = ico("i-chev-left", "ico-sm");
        $("#btnCollapseRight").innerHTML = ico("i-chev-right", "ico-sm");
        state.zoom = 100;
        renderTimeline();
        break;
      case "import-srt":
        simulateJob("Mô phỏng: Đang nhập phụ đề SRT…", 900, () => setJobStatus("Đã nhập file SRT (mô phỏng)", 100, true, "ok"));
        break;
      case "export-sub":
        setJobStatus("Đã xuất phụ đề SRT/LRC/ASS (mô phỏng)", 100, true, "ok");
        break;
      case "open-roles":
        simulateJob("Mô phỏng: Đang phân tích cao độ để gợi ý vai…", 1200, () => setJobStatus("Đã gợi ý vai Nam / Nữ (mô phỏng)", 100, true, "ok"));
        break;
      case "undo":
      case "redo":
        setJobStatus(cmd === "undo" ? "Đã hoàn tác thao tác gần nhất" : "Đã làm lại", 0, false);
        break;
    }
  }

  // ---------- 2. TOPBAR & NAVIGATION ----------
  $$(".page-switch button").forEach(btn => {
    btn.addEventListener("click", () => switchPage(btn.dataset.page));
  });

  function switchPage(page) {
    state.page = page;
    $$(".page-switch button").forEach(b => b.classList.toggle("on", b.dataset.page === page));
    $$(".page-view").forEach(v => v.classList.toggle("hidden", v.id !== "page-" + page));
    $("#topbarProjectMeta").classList.toggle("hidden", page !== "karaoke");
    $(".topbar-actions").classList.toggle("hidden", page !== "karaoke");
  }

  $("#projectTitleInput").addEventListener("input", (e) => {
    state.project.name = e.target.value;
    state.project.saved = false;
    renderTopbar();
  });

  $("#btnAutoAlign").addEventListener("click", () => runCommand("auto-align"));
  $("#btnStartTap").addEventListener("click", () => toggleTapping());
  $("#btnOpenExport").addEventListener("click", () => $("#dlgExport").showModal());
  $("#btnDismissError").addEventListener("click", () => setDemoMode("ready"));

  function renderTopbar() {
    $("#projectTitleInput").value = state.project.name;
    const badge = $("#saveBadge");
    badge.textContent = state.project.saved ? "● Đã lưu" : "● Chưa lưu";
    badge.classList.toggle("warn", !state.project.saved);
  }

  // ---------- 3. BẢNG TRÁI (LỜI & MEDIA) ----------
  $$("[data-left-tab]").forEach(b => {
    b.addEventListener("click", () => switchLeftTab(b.dataset.leftTab));
  });

  function switchLeftTab(tab) {
    $$("[data-left-tab]").forEach(b => b.classList.toggle("on", b.dataset.leftTab === tab));
    $("#leftTabLyrics").classList.toggle("hidden", tab !== "lyrics");
    $("#leftTabMedia").classList.toggle("hidden", tab !== "media");
  }

  function toggleLeftPanel() {
    const collapsed = $("#leftPanel").classList.toggle("is-collapsed");
    $("#studioGrid").classList.toggle("left-collapsed", collapsed);
    $("#btnCollapseLeft").innerHTML = ico(collapsed ? "i-chev-right" : "i-chev-left", "ico-sm");
  }
  function toggleRightPanel() {
    const collapsed = $("#rightPanel").classList.toggle("is-collapsed");
    $("#studioGrid").classList.toggle("right-collapsed", collapsed);
    $("#btnCollapseRight").innerHTML = ico(collapsed ? "i-chev-left" : "i-chev-right", "ico-sm");
  }
  $("#btnCollapseLeft").addEventListener("click", toggleLeftPanel);
  $("#btnCollapseRight").addEventListener("click", toggleRightPanel);

  $("#btnToggleLyricsEdit").addEventListener("click", () => {
    const box = $("#lyricsEditBox");
    const open = box.classList.toggle("hidden");
    if (!open) {
      $("#lyricsTextarea").value = state.project.lines.map(l => l.words.map(w => w.t).join(" ")).join("\n");
    }
  });
  $("#btnCancelLyrics").addEventListener("click", () => $("#lyricsEditBox").classList.add("hidden"));
  $("#btnApplyLyrics").addEventListener("click", () => {
    const raw = $("#lyricsTextarea").value.split(/\r?\n/).map(s => s.trim()).filter(Boolean);
    let t = 1.5;
    state.project.lines = raw.map((line, i) => {
      const words = line.split(/\s+/).map(w => {
        const s = t;
        const e = +(t + 0.55).toFixed(2);
        t = +(e + 0.08).toFixed(2);
        return { t: w, s, e };
      });
      t += 0.4;
      return { id: i + 1, role: "all", locked: false, start: words[0]?.s || 0, words };
    });
    state.project.duration = Math.max(20, +(t + 3).toFixed(1));
    state.project.saved = false;
    $("#lyricsEditBox").classList.add("hidden");
    renderAll();
  });

  $("#btnPickRef").addEventListener("click", () => {
    state.project.hasReference = true;
    state.project.referenceFile = "BanHat_Moi_Vocal.wav";
    renderMedia();
    setJobStatus("Đã nạp bản hát tham chiếu (mô phỏng)", 100, true, "ok");
  });
  $("#btnPickBeat").addEventListener("click", () => {
    state.project.hasBeat = true;
    state.project.beatFile = "BeatGoc_Moi.wav";
    renderMedia();
    setJobStatus("Đã nạp beat gốc (mô phỏng)", 100, true, "ok");
  });
  $("#btnRunBacking").addEventListener("click", () => {
    simulateJob("Mô phỏng: Đang tách giọng bè (Mel-RoFormer)…", 2000, () => {
      state.project.hasBacking = true;
      state.project.backingAccepted = true;
      renderMedia();
      setJobStatus("Đã tách & duyệt bản Beat + bè (mô phỏng)", 100, true, "ok");
    });
  });

  function renderLyrics() {
    const lines = state.project.lines;
    const lockedCount = lines.filter(l => l.locked).length;
    $("#lineCountLabel").textContent = lines.length ? `${lines.length} câu · ${lockedCount} đã khóa` : "Chưa có lời";

    const list = $("#lyricList");
    if (!lines.length) {
      list.innerHTML = `<p class="hint">Chưa có câu nào. Bấm "Sửa toàn bộ" để dán lời bài hát.</p>`;
      return;
    }
    list.innerHTML = lines.map((line, idx) => {
      const text = line.words.map(w => w.t).join(" ");
      const active = line.id === state.selectedLineId ? "active" : "";
      const roleColor = ROLE_COLOR[line.role] || ROLE_COLOR.all;
      return `
        <div class="lyric-row ${active}" data-line-id="${line.id}">
          <span class="lyric-num">${idx + 1}</span>
          <span class="role-dot" style="background:${roleColor}" title="Vai: ${line.role}"></span>
          <span class="lyric-text">${text}</span>
          <span class="lyric-time">${fmtTime(line.start).slice(0, 5)}</span>
          <button class="lock-btn ${line.locked ? "locked" : ""}" data-lock-id="${line.id}" title="Khóa / mở khóa câu">${ico(line.locked ? "i-lock" : "i-unlock", "ico-sm")}</button>
        </div>`;
    }).join("");

    $$("[data-line-id]", list).forEach(row => {
      row.addEventListener("click", (e) => {
        if (e.target.closest("[data-lock-id]")) return;
        const id = Number(row.dataset.lineId);
        const line = state.project.lines.find(l => l.id === id);
        if (!line) return;
        state.selectedLineId = id;
        state.selectedWordIdx = 0;
        state.currentTime = line.start;
        renderAll();
      });
    });

    $$("[data-lock-id]", list).forEach(btn => {
      btn.addEventListener("click", () => {
        const line = state.project.lines.find(l => l.id === Number(btn.dataset.lockId));
        if (line) {
          line.locked = !line.locked;
          renderLyrics();
        }
      });
    });
  }

  function renderMedia() {
    const p = state.project;
    $("#refMediaCard").classList.toggle("loaded", p.hasReference);
    $("#refBadge").textContent = p.hasReference ? "Đã nạp" : "Chưa nạp";
    $("#refBadge").className = "badge " + (p.hasReference ? "ok" : "warn");
    $("#refFileName").textContent = p.hasReference ? p.referenceFile : "Chưa chọn file";

    $("#beatMediaCard").classList.toggle("loaded", p.hasBeat);
    $("#beatBadge").textContent = p.hasBeat ? "Đã nạp" : "Chưa nạp";
    $("#beatBadge").className = "badge " + (p.hasBeat ? "ok" : "warn");
    $("#beatFileName").textContent = p.hasBeat ? p.beatFile : "Chưa chọn file";

    $("#backingStatusText").textContent = p.hasBacking
      ? "Đã duyệt bản Beat + bè. Bật nút Bè ở thanh phát để nghe thử."
      : "Nạp bản hát và beat để tách giọng bè.";
  }

  // ---------- 4. PREVIEW & TRANSPORT ----------
  $("#btnPlay").addEventListener("click", togglePlayback);
  $("#btnToggleAspect").addEventListener("click", () => {
    state.project.style.aspect = state.project.style.aspect === "landscape" ? "portrait" : "landscape";
    $("#propAspect").value = state.project.style.aspect;
    renderPreview();
  });
  $("#btnBackingToggle").addEventListener("click", () => {
    state.backingOn = !state.backingOn;
    $("#btnBackingLabel").textContent = state.backingOn ? "Bè · Bật" : "Bè · Tắt";
    $("#btnBackingToggle").classList.toggle("on", state.backingOn);
  });
  $("#speedSelect").addEventListener("change", (e) => { state.speed = Number(e.target.value); });

  $("#btnEmptyLoadSample").addEventListener("click", () => setDemoMode("ready"));
  $("#btnEmptyOpenMedia").addEventListener("click", () => switchLeftTab("media"));

  function togglePlayback() {
    if (!state.project.lines.length) return;
    if (state.playing) stopPlayback();
    else startPlayback();
  }
  function startPlayback() {
    state.playing = true;
    $("#btnPlay").innerHTML = ico("i-pause");
    clearInterval(state.playTimer);
    state.playTimer = setInterval(() => {
      state.currentTime += 0.08 * state.speed;
      if (state.currentTime >= state.project.duration) {
        state.currentTime = 0;
        stopPlayback();
      }
      renderPreview();
      updatePlayheadPosition();
    }, 80);
  }
  function stopPlayback() {
    state.playing = false;
    $("#btnPlay").innerHTML = ico("i-play");
    clearInterval(state.playTimer);
  }

  function renderPreview() {
    const p = state.project;
    const st = p.style;
    const hasData = p.lines.length > 0;

    $("#stageEmpty").classList.toggle("hidden", hasData);
    $("#stageFrame").classList.toggle("portrait", st.aspect === "portrait");
    $("#stageFrame").style.background = st.bgType === "color"
      ? st.bgColor
      : "linear-gradient(155deg, #271e38 0%, #101b30 58%, #102626 100%)";

    $("#previewMetaLabel").textContent = `${st.aspect === "portrait" ? "9:16" : "16:9"} · ${st.resolution.toUpperCase()} · Beat gốc`;
    $("#timecode").textContent = `${fmtTime(state.currentTime)} / ${fmtTime(p.duration)}`;

    $("#stageTitleBox").classList.toggle("hidden", !st.showTitle || !hasData);
    $("#stageSongTitle").textContent = st.titleText;
    $("#stageSongArtist").textContent = st.titleArtist;

    $("#stageLogo").classList.toggle("hidden", !st.showLogo || !hasData);
    $("#stageLogo").textContent = st.logoText;

    $("#stageCountdown").classList.toggle("hidden", !(hasData && st.countdown !== "none" && state.currentTime < 2.0));

    const lyrBox = $("#stageLyrics");
    if (!hasData) { lyrBox.innerHTML = ""; return; }

    lyrBox.style.fontFamily = st.font;
    lyrBox.style.fontSize = st.fontSize + "px";
    lyrBox.style.textAlign = st.align;
    lyrBox.style.gap = st.lineGap + "px";

    // Tìm câu hiện tại theo currentTime
    let idx = p.lines.findIndex(l => {
      const last = l.words[l.words.length - 1];
      return state.currentTime <= (last ? last.e + 0.3 : l.start + 3);
    });
    if (idx === -1) idx = p.lines.length - 1;

    const cur = p.lines[idx];
    const next = p.lines[idx + 1];

    const renderLineHtml = (line, isNext) => {
      if (!line) return "";
      const roleTint = p.singingMode === "duet" && line.role !== "all" ? ROLE_COLOR[line.role] : st.sungColor;
      const wordsHtml = line.words.map(w => {
        const sung = state.currentTime >= w.e || (state.currentTime >= w.s && state.currentTime <= w.e);
        const color = sung ? roleTint : st.unsungColor;
        return `<span class="stage-word" style="color:${color}">${w.t}</span>`;
      }).join(" ");
      return `<div class="stage-line ${isNext ? "next" : ""}">${wordsHtml}</div>`;
    };

    lyrBox.innerHTML = renderLineHtml(cur, false) + renderLineHtml(next, true);
  }

  // ---------- 5. INSPECTOR (TAB DỌC ICON) ----------
  $$(".insp-tab").forEach(tab => {
    tab.addEventListener("click", () => {
      state.inspTab = tab.dataset.insp;
      $$(".insp-tab").forEach(t => t.classList.toggle("on", t === tab));
      $$("[data-insp-panel]").forEach(p => p.classList.toggle("hidden", p.dataset.inspPanel !== state.inspTab));
      $("#inspectorHeadTitle").textContent = INSP_TITLES[state.inspTab] || "Thuộc tính";
    });
  });

  function renderInspector() {
    const p = state.project;
    const st = p.style;

    // Tab Selection
    const line = p.lines.find(l => l.id === state.selectedLineId);
    const word = line?.words[state.selectedWordIdx];
    $("#noWordSelected").classList.toggle("hidden", Boolean(word));
    $("#wordSelectedForm").classList.toggle("hidden", !word);
    if (word && line) {
      $("#selWordText").value = word.t;
      $("#selWordStart").value = word.s;
      $("#selWordEnd").value = word.e;
      $("#selLineRole").value = line.role;
    }

    // Tab Style, Duet, Title, Bg, Logo
    $("#propFont").value = st.font;
    $("#propFontSize").value = st.fontSize;
    $("#lblFontSize").textContent = st.fontSize;
    $("#propAlign").value = st.align;
    $("#propSungColor").value = st.sungColor;
    $("#propUnsungColor").value = st.unsungColor;
    $("#propLineGap").value = st.lineGap;
    $("#propSingingMode").value = p.singingMode;
    $("#propShowTitle").checked = st.showTitle;
    $("#propTitleText").value = st.titleText;
    $("#propTitleArtist").value = st.titleArtist;
    $("#propBgType").value = st.bgType;
    $("#propBgColor").value = st.bgColor;
    $("#propAspect").value = st.aspect;
    $("#propShowLogo").checked = st.showLogo;
    $("#propLogoText").value = st.logoText;
    $("#propCountdown").value = st.countdown;

    // Presets
    $("#presetGrid").innerHTML = window.MOCK_DATA.presets.map(pr => `
      <button class="preset-item" data-preset-id="${pr.id}">
        <div style="font-weight:600;color:${pr.sung}">${pr.name}</div>
        <div class="hint" style="margin:2px 0 0">${pr.desc}</div>
      </button>`).join("");
    $$("[data-preset-id]").forEach(b => {
      b.addEventListener("click", () => {
        const pr = window.MOCK_DATA.presets.find(x => x.id === b.dataset.presetId);
        if (!pr) return;
        st.sungColor = pr.sung;
        st.unsungColor = pr.unsung;
        st.font = pr.font;
        renderInspector();
        renderPreview();
      });
    });
  }

  // Live bindings Inspector -> Preview
  const bindProp = (id, fn) => $(id).addEventListener("input", (e) => { fn(e.target); state.project.saved = false; renderTopbar(); renderPreview(); });
  bindProp("#propFont", el => state.project.style.font = el.value);
  bindProp("#propFontSize", el => { state.project.style.fontSize = Number(el.value); $("#lblFontSize").textContent = el.value; });
  bindProp("#propAlign", el => state.project.style.align = el.value);
  bindProp("#propSungColor", el => state.project.style.sungColor = el.value);
  bindProp("#propUnsungColor", el => state.project.style.unsungColor = el.value);
  bindProp("#propLineGap", el => state.project.style.lineGap = Number(el.value));
  bindProp("#propSingingMode", el => state.project.singingMode = el.value);
  bindProp("#propShowTitle", el => state.project.style.showTitle = el.checked);
  bindProp("#propTitleText", el => state.project.style.titleText = el.value);
  bindProp("#propTitleArtist", el => state.project.style.titleArtist = el.value);
  bindProp("#propBgType", el => state.project.style.bgType = el.value);
  bindProp("#propBgColor", el => state.project.style.bgColor = el.value);
  bindProp("#propAspect", el => state.project.style.aspect = el.value);
  bindProp("#propShowLogo", el => state.project.style.showLogo = el.checked);
  bindProp("#propLogoText", el => state.project.style.logoText = el.value);
  bindProp("#propCountdown", el => state.project.style.countdown = el.value);

  $("#selWordStart").addEventListener("change", (e) => {
    const line = state.project.lines.find(l => l.id === state.selectedLineId);
    const word = line?.words[state.selectedWordIdx];
    if (word) { word.s = Number(e.target.value); renderTimeline(); }
  });
  $("#selWordEnd").addEventListener("change", (e) => {
    const line = state.project.lines.find(l => l.id === state.selectedLineId);
    const word = line?.words[state.selectedWordIdx];
    if (word) { word.e = Number(e.target.value); renderTimeline(); }
  });
  $("#selLineRole").addEventListener("change", (e) => {
    const line = state.project.lines.find(l => l.id === state.selectedLineId);
    if (line) { line.role = e.target.value; renderAll(); }
  });
  $("#btnLockLine").addEventListener("click", () => {
    const line = state.project.lines.find(l => l.id === state.selectedLineId);
    if (line) { line.locked = !line.locked; renderLyrics(); }
  });
  $("#btnRealignLine").addEventListener("click", () => runCommand("auto-align"));
  $("#btnOpenAnchorsFromInsp").addEventListener("click", () => $("#dlgAnchors").showModal());
  $("#btnAutoRolesInsp").addEventListener("click", () => runCommand("open-roles"));
  $("#btnSwapRoles").addEventListener("click", () => {
    state.project.lines.forEach(l => {
      if (l.role === "male") l.role = "female";
      else if (l.role === "female") l.role = "male";
    });
    renderAll();
  });

  // ---------- 6. TIMELINE & TAPPING ----------
  $("#btnZoomIn").addEventListener("click", () => { state.zoom = Math.min(240, state.zoom + 20); renderTimeline(); });
  $("#btnZoomOut").addEventListener("click", () => { state.zoom = Math.max(60, state.zoom - 20); renderTimeline(); });
  $("#btnToggleWaves").addEventListener("click", () => runCommand("toggle-waves"));
  $("#btnFollowPlayhead").addEventListener("click", () => {
    state.followPlayhead = !state.followPlayhead;
    $("#btnFollowPlayhead").classList.toggle("on", state.followPlayhead);
  });
  $("#btnLinkEdges").addEventListener("click", () => {
    state.linkEdges = !state.linkEdges;
    $("#btnLinkEdges").classList.toggle("on", state.linkEdges);
  });

  function renderTimeline() {
    $("#zoomLabel").textContent = state.zoom + "%";
    const canvas = $("#tlCanvas");
    const p = state.project;
    if (!p.lines.length) {
      canvas.innerHTML = `<div style="padding:28px;text-align:center" class="hint">Chưa có dữ liệu timeline. Hãy nạp bài hát hoặc dán lời để bắt đầu.</div>`;
      return;
    }

    canvas.style.width = state.zoom + "%";
    const dur = p.duration || 40;
    const ticks = [];
    for (let s = 0; s <= dur; s += 5) {
      ticks.push(`<div class="tl-tick" style="left:${(s / dur) * 100}%">${fmtTime(s).slice(0, 5)}</div>`);
    }

    const wordsHtml = [];
    p.lines.forEach(line => {
      line.words.forEach((w, wi) => {
        const left = (w.s / dur) * 100;
        const width = Math.max(1.2, ((w.e - w.s) / dur) * 100);
        const sel = (line.id === state.selectedLineId && wi === state.selectedWordIdx) ? "selected" : "";
        const bg = ROLE_COLOR[line.role] || ROLE_COLOR.all;
        wordsHtml.push(`<div class="tl-word ${sel}" data-lid="${line.id}" data-wi="${wi}" style="left:${left}%;width:${width}%;background:${bg}">${w.t}</div>`);
      });
    });

    const waveCss = state.showWaves ? "" : "hidden";
    const phLeft = `${(state.currentTime / dur) * 100}%`;

    canvas.innerHTML = `
      <div class="tl-ruler" id="tlRuler">
        <div style="width:var(--w-track-label);flex:none;border-right:1px solid var(--c-line);padding:0 8px;font-size:10px;color:var(--c-text-3);line-height:22px">Thời gian</div>
        <div style="flex:1;position:relative" id="tlRulerTrack">${ticks.join("")}<div class="tl-playhead" id="ph0" style="left:${phLeft}"></div></div>
      </div>
      <div class="tl-row">
        <div class="tl-row-head">Bản hát (quy đổi)</div>
        <div class="tl-lane"><div class="tl-wave ${waveCss}" style="--wave-c:var(--c-wave-ref)"></div><div class="tl-playhead" id="ph1" style="left:${phLeft}"></div></div>
      </div>
      <div class="tl-row">
        <div class="tl-row-head">Beat gốc</div>
        <div class="tl-lane"><div class="tl-wave ${waveCss}" style="--wave-c:var(--c-wave-beat)"></div><div class="tl-playhead" id="ph2" style="left:${phLeft}"></div></div>
      </div>
      <div class="tl-row" style="flex:1.3">
        <div class="tl-row-head">Lời từng từ</div>
        <div class="tl-lane">${wordsHtml.join("")}<div class="tl-playhead" id="ph3" style="left:${phLeft}"></div></div>
      </div>
    `;

    $("#tlRulerTrack").addEventListener("click", (e) => {
      const rect = e.currentTarget.getBoundingClientRect();
      const ratio = Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width));
      state.currentTime = +(ratio * dur).toFixed(2);
      renderPreview();
      updatePlayheadPosition();
    });

    $$(".tl-word", canvas).forEach(el => {
      el.addEventListener("click", () => {
        state.selectedLineId = Number(el.dataset.lid);
        state.selectedWordIdx = Number(el.dataset.wi);
        const line = p.lines.find(l => l.id === state.selectedLineId);
        const w = line?.words[state.selectedWordIdx];
        if (w) state.currentTime = w.s;
        renderAll();
      });
    });
  }

  function updatePlayheadPosition() {
    const dur = state.project.duration || 40;
    const pct = `${(state.currentTime / dur) * 100}%`;
    ["ph0", "ph1", "ph2", "ph3"].forEach(id => {
      const el = document.getElementById(id);
      if (el) el.style.left = pct;
    });
  }

  // Gõ nhịp Space mô phỏng
  function toggleTapping() {
    if (!state.project.lines.length) return;
    state.tapping = !state.tapping;
    $("#tapBar").classList.toggle("hidden", !state.tapping);
    $("#btnStartTap").classList.toggle("on", state.tapping);
    if (state.tapping) {
      state.tapIndex = 0;
      updateTapLabel();
      if (!state.playing) startPlayback();
    }
  }
  function updateTapLabel() {
    const flat = state.project.lines.flatMap(l => l.words);
    const w = flat[state.tapIndex];
    $("#tapNextWord").textContent = w ? w.t : "Đã hết bài!";
  }
  function stepTap() {
    const flat = state.project.lines.flatMap(l => l.words);
    if (state.tapIndex < flat.length) {
      flat[state.tapIndex].s = +state.currentTime.toFixed(2);
      flat[state.tapIndex].e = +(state.currentTime + 0.55).toFixed(2);
      state.tapIndex++;
      updateTapLabel();
      renderTimeline();
    }
  }
  $("#btnTapStep").addEventListener("click", stepTap);
  $("#btnTapFinish").addEventListener("click", () => { if (state.tapping) toggleTapping(); });

  // ---------- 7. TÁCH NHẠC & CÀI ĐẶT MODEL ----------
  const runSeparation = (withBacking) => {
    $("#sepResultBox").classList.add("hidden");
    simulateJob(`Mô phỏng: Đang tách nhạc (${$("#sepModel").value})…`, 1800, () => {
      $("#sepResultBox").classList.remove("hidden");
      $("#sepBackingCard").style.opacity = withBacking ? "1" : "0.45";
      setJobStatus("Tách nhạc xong · 3 file WAV sẵn sàng (mô phỏng)", 100, true, "ok");
    });
  };
  $("#btnSepNoBacking").addEventListener("click", () => runSeparation(false));
  $("#btnSepWithBacking").addEventListener("click", () => runSeparation(true));
  $$("[data-sep-play]").forEach(btn => {
    btn.addEventListener("click", () => setJobStatus(`Đang phát thử bản ${btn.dataset.sepPlay} (mô phỏng)`, 0, false));
  });

  function renderModels() {
    $("#modelList").innerHTML = window.MOCK_DATA.models.map(m => {
      const badgeCls = m.status === "ok" ? "ok" : m.status === "missing" ? "warn" : "err";
      const badgeTxt = m.status === "ok" ? "Sẵn sàng" : m.status === "missing" ? "Chưa tải" : "Lỗi SHA-256";
      return `
        <div style="display:flex;align-items:center;gap:12px;padding:10px 0;border-bottom:1px solid var(--c-line)">
          <div style="flex:1">
            <div><strong>${m.name}</strong> <span class="hint">· ${m.group} · ${m.size}</span></div>
            <div class="hint" style="font-family:var(--font-mono)">${m.path} — ${m.note}</div>
          </div>
          <span class="badge ${badgeCls}">${badgeTxt}</span>
          ${m.status !== "ok" ? `<button class="btn sm" data-dl-model="${m.id}">Tải lại (mô phỏng)</button>` : ""}
        </div>`;
    }).join("");

    $$("[data-dl-model]").forEach(b => {
      b.addEventListener("click", () => {
        const m = window.MOCK_DATA.models.find(x => x.id === b.dataset.dlModel);
        simulateJob(`Mô phỏng: Đang tải ${m.name}…`, 1500, () => {
          m.status = "ok"; m.note = "Đã kiểm tra SHA-256";
          renderModels();
          setJobStatus(`Đã tải ${m.name} (mô phỏng)`, 100, true, "ok");
        });
      });
    });
  }
  $("#btnRefreshModels").addEventListener("click", () => {
    renderModels();
    setJobStatus("Đã kiểm tra danh sách model cục bộ", 0, false);
  });
  $("#btnCheckUpdate").addEventListener("click", () => {
    $("#updateStatusText").textContent = "Đang kiểm tra… (mô phỏng)";
    setTimeout(() => { $("#updateStatusText").textContent = "Đã có bản vá 1.1.16 (mô phỏng) — giữ nguyên project và model."; }, 700);
  });

  // ---------- 8. DIALOGS & JOB SIMULATION ----------
  $$("[data-close]").forEach(btn => btn.addEventListener("click", () => btn.closest("dialog").close()));

  function openProjectsDialog() {
    $("#projectListContainer").innerHTML = window.MOCK_DATA.projects.map(p => `
      <div class="card-box" style="display:flex;align-items:center;gap:10px;padding:10px 12px;background:var(--c-inset)">
        <input type="checkbox" checked>
        <div style="flex:1">
          <strong>${p.name}</strong>
          <div class="hint" style="margin:0">${p.artist} · ${p.lines.length} câu · ${p.singingMode === "duet" ? "Song ca" : "Đơn ca"}</div>
        </div>
        <button class="btn sm" data-open-proj="${p.id}">Mở</button>
      </div>`).join("");
    $$("[data-open-proj]").forEach(b => {
      b.addEventListener("click", () => {
        const found = window.MOCK_DATA.projects.find(x => x.id === b.dataset.openProj);
        if (found) {
          state.project = structuredClone(found);
          state.currentTime = 2.0;
          state.selectedLineId = 1;
          state.selectedWordIdx = 0;
          $("#dlgProjects").close();
          setDemoMode("ready");
        }
      });
    });
    $("#dlgProjects").showModal();
  }

  $("#btnRunBatch").addEventListener("click", () => {
    $("#dlgProjects").close();
    simulateJob("Mô phỏng: Đang dựng 2 project theo lô…", 2000, () => setJobStatus("Hoàn tất dựng theo lô (mô phỏng)", 100, true, "ok"));
  });

  $("#btnPreviewRender").addEventListener("click", () => {
    simulateJob("Mô phỏng: Đang render thử 8 giây…", 1400, () => $("#expResultOk").classList.remove("hidden"));
  });
  $("#btnRunRender").addEventListener("click", () => {
    $("#expResultOk").classList.add("hidden");
    simulateJob("Mô phỏng: Đang dựng video MP4 1080p…", 2000, () => {
      $("#expResultOk").classList.remove("hidden");
      setJobStatus("Xuất video thành công (mô phỏng)", 100, true, "ok");
    });
  });

  function setJobStatus(text, pct, showBar, tone = "") {
    $("#statusJobText").textContent = text;
    $("#statusBarTrack").classList.toggle("hidden", !showBar);
    $("#statusBarFill").style.width = pct + "%";
    $("#statusBarFill").className = "status-bar-fill " + tone;
    $("#statusJobPct").textContent = showBar ? pct + "%" : "";
  }

  function simulateJob(label, durationMs, onDone) {
    clearInterval(state.jobTimer);
    let pct = 8;
    setJobStatus(label, pct, true);
    const stepTime = Math.max(80, durationMs / 12);
    state.jobTimer = setInterval(() => {
      pct = Math.min(100, pct + 9);
      setJobStatus(label, pct, true);
      if (pct >= 100) {
        clearInterval(state.jobTimer);
        if (onDone) onDone();
      }
    }, stepTime);
  }

  // Phím tắt toàn cục
  document.addEventListener("keydown", (e) => {
    if (["INPUT", "TEXTAREA", "SELECT"].includes(document.activeElement.tagName)) return;
    if (e.code === "Space") {
      e.preventDefault();
      if (state.tapping) stepTap();
      else togglePlayback();
    } else if (e.ctrlKey && e.key.toLowerCase() === "e") {
      e.preventDefault();
      $("#dlgExport").showModal();
    }
  });

  function renderAll() {
    renderTopbar();
    renderLyrics();
    renderMedia();
    renderPreview();
    renderInspector();
    renderTimeline();
    renderModels();
  }

  renderAll();
})();
