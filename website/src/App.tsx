import { useEffect, useState } from "react";

const REPOSITORY = "https://github.com/ahmetfsyn/carambola";
const RELEASES = `${REPOSITORY}/releases/latest`;

type Language = "en" | "tr";

const copy = {
  en: {
    navFeatures: "Features", navHow: "How it works", navPrivacy: "Privacy", navDownload: "Download",
    eyebrow: "A little tool for the things you need to show",
    title: <>A clearer way to<br /><span>show what you mean.</span></>,
    intro: "Capture a part of your screen, mark it up, and send it on. Carambola keeps the whole thing quick and close at hand.",
    download: "Get Carambola", github: "View on GitHub", platform: "Windows 10/11 · 64-bit", preview: "A closer look",
    previewTitle: "Just the part that matters.", previewSub: "Select · annotate · share",
    selection: "A tiny detail, made clear.", toolSelect: "Select", toolPen: "Pen", toolText: "Text", toolMosaic: "Mosaic", toolCopy: "Copy image",
    sectionLabel: "MADE FOR THE MOMENT", featureTitle: <>Everything you need.<br /><span>Nothing in the way.</span></>,
    featureIntro: "A focused set of tools for capturing a detail and making it easy to understand.",
    features: [
      ["01", "Quick screen capture", "Start a capture from your keyboard shortcut or the system tray."],
      ["02", "Mark it up", "Draw freehand or add arrows, shapes, and text to your selection."],
      ["03", "Keep details private", "Pixelate sensitive areas with the mosaic tool before sharing."],
      ["04", "Text recognition", "Copy text from a capture with OCR. Available on Windows only."],
      ["05", "Copy or save", "Copy the finished image to your clipboard or save it as a PNG."],
      ["06", "Your colors, your way", "Choose from eight annotation colors and adjustable stroke widths."],
    ],
    workflowLabel: "THREE SMALL STEPS", workflowTitle: "From screen to shared in seconds.",
    steps: [["01", "Start a capture", "Use your keyboard shortcut or choose Capture Screen from the tray."], ["02", "Select and annotate", "Drag to choose an area. Add a note, arrow, shape, or mosaic."], ["03", "Copy or save", "Copy the image straight to your clipboard or save a PNG." ]],
    privacyLabel: "YOUR SCREEN STAYS YOURS", privacyTitle: <>Made to stay<br /><span>on your computer.</span></>, privacyBody: "Carambola captures and edits screenshots locally. It does not upload them to a cloud service. Copy or save only when you choose.", privacyNote: "No account. No cloud upload feature.",
    downloadLabel: "READY WHEN YOU ARE", downloadTitle: "Make your next screenshot simpler.", downloadBody: "Download Carambola for Windows 10/11 x64. Releases include a standalone EXE, portable ZIP, and SHA-256 checksums.", releases: "Download for Windows", source: "Explore the source",
    buildsTitle: "Current build", mac: "macOS", macText: "Not a supported release target. No verified macOS build is available.", windows: "Windows 10/11 x64", windowsText: "Portable EXE and ZIP available. No installer; the executable is unsigned.", linux: "Linux", linuxText: "Not a supported release target. No verified Linux build is available.",
    footerLine: "A small tool for clearer communication.", license: "No license has been selected yet.", made: "Made with care.",
    ariaLanguage: "Choose language", ariaMock: "Illustrative Carambola interface mockup, not an app screenshot",
    ready: "READY", demoDay: "TUESDAY", captureTagline: "CAPTURE THE LITTLE THINGS", scrollExplore: "SCROLL TO EXPLORE",
    localByDesign: "LOCAL BY DESIGN", yourDevice: "YOUR DEVICE", offlineFirst: "OFFLINE FIRST",
  },
  tr: {
    navFeatures: "Özellikler", navHow: "Nasıl çalışır", navPrivacy: "Gizlilik", navDownload: "İndir",
    eyebrow: "Göstermek istedikleriniz için küçük bir araç",
    title: <>Ne demek istediğinizi<br /><span>daha net gösterin.</span></>,
    intro: "Ekranınızdan bir alan seçin, üzerine not ekleyin ve paylaşın. Carambola bu işi hızlı ve elinizin altında tutar.",
    download: "Carambola’yı alın", github: "GitHub’da görüntüle", platform: "Windows 10/11 · 64 bit", preview: "Yakından bakın",
    previewTitle: "Önemli olanı seçin.", previewSub: "Seç · not ekle · paylaş",
    selection: "Küçük bir ayrıntı, artık daha net.", toolSelect: "Seç", toolPen: "Kalem", toolText: "Metin", toolMosaic: "Mozaik", toolCopy: "Görseli kopyala",
    sectionLabel: "İHTİYACINIZ OLDUĞUNDA", featureTitle: <>İhtiyacınız olan her şey.<br /><span>Önünüze çıkmadan.</span></>,
    featureIntro: "Bir ayrıntıyı yakalayıp anlaşılır kılmak için gereken temel araçlar.",
    features: [
      ["01", "Hızlı ekran görüntüsü", "Klavye kısayoluyla veya sistem tepsisinden yakalamayı başlatın."],
      ["02", "Üzerine not ekleyin", "Serbest çizim yapın; seçiminize ok, şekil ve metin ekleyin."],
      ["03", "Ayrıntıları gizleyin", "Paylaşmadan önce hassas alanları mozaik aracıyla pikselleştirin."],
      ["04", "Metin tanıma", "OCR ile görüntüdeki metni kopyalayın. Yalnızca Windows’ta kullanılabilir."],
      ["05", "Kopyalayın veya kaydedin", "Görseli panoya kopyalayın ya da PNG olarak kaydedin."],
      ["06", "Renkleri siz seçin", "Sekiz açıklama renginden ve farklı çizgi kalınlıklarından seçim yapın."],
    ],
    workflowLabel: "ÜÇ KOLAY ADIM", workflowTitle: "Ekrandan paylaşıma, saniyeler içinde.",
    steps: [["01", "Yakalamayı başlatın", "Klavye kısayolunu kullanın veya tepsiden Ekranı Yakala’yı seçin."], ["02", "Alanı seçip not ekleyin", "Bir alan seçmek için sürükleyin. Not, ok, şekil veya mozaik ekleyin."], ["03", "Kopyalayın veya kaydedin", "Görseli doğrudan panoya kopyalayın ya da PNG olarak kaydedin." ]],
    privacyLabel: "EKRANINIZ SİZE AİT", privacyTitle: <>Bilgisayarınızda<br /><span>kalacak şekilde tasarlandı.</span></>, privacyBody: "Carambola ekran görüntülerini yerel olarak alır ve düzenler. Görselleri bulut hizmetine yüklemez. Yalnızca siz istediğinizde kopyalar veya kaydeder.", privacyNote: "Hesap gerekmez. Buluta yükleme özelliği yoktur.",
    downloadLabel: "HAZIR OLDUĞUNUZDA", downloadTitle: "Bir sonraki ekran görüntünüzü kolaylaştırın.", downloadBody: "Carambola'yı Windows 10/11 x64 için indirin. Sürümlerde tek başına EXE, taşınabilir ZIP ve SHA-256 sağlama toplamları bulunur.", releases: "Windows için indir", source: "Kaynak kodu inceleyin",
    buildsTitle: "Mevcut derleme", mac: "macOS", macText: "Desteklenen bir sürüm hedefi değildir. Doğrulanmış macOS derlemesi yoktur.", windows: "Windows 10/11 x64", windowsText: "Taşınabilir EXE ve ZIP mevcut. Yükleyici yoktur; yürütülebilir dosya imzalı değildir.", linux: "Linux", linuxText: "Desteklenen bir sürüm hedefi değildir. Doğrulanmış Linux derlemesi yoktur.",
    footerLine: "Daha net iletişim için küçük bir araç.", license: "Henüz bir lisans seçilmedi.", made: "Özenle geliştirildi.",
    ariaLanguage: "Dil seçin", ariaMock: "Carambola arayüzünü gösteren örnek tasarım, uygulama ekran görüntüsü değildir",
    ready: "HAZIR", demoDay: "SALI", captureTagline: "KÜÇÜK AYRINTILARI YAKALAYIN", scrollExplore: "KEŞFETMEK İÇİN KAYDIRIN",
    localByDesign: "YEREL ÇALIŞIR", yourDevice: "CİHAZINIZ", offlineFirst: "ÖNCE ÇEVRİMDIŞI",
  },
} as const;

function Arrow({ diagonal = false }: { diagonal?: boolean }) {
  return diagonal ? <svg aria-hidden="true" viewBox="0 0 16 16"><path d="M4 12 12 4M5 4h7v7" /></svg> : <svg aria-hidden="true" viewBox="0 0 16 16"><path d="M2.5 8h10m-4-4 4 4-4 4" /></svg>;
}

function App() {
  const [language, setLanguage] = useState<Language>(() => {
    try { return localStorage.getItem("carambola-language") === "tr" ? "tr" : "en"; }
    catch { return "en"; }
  });
  const t = copy[language];

  useEffect(() => {
    document.documentElement.lang = language;
    try { localStorage.setItem("carambola-language", language); } catch { /* Storage can be unavailable in private contexts. */ }
  }, [language]);

  return <>
    <header className="site-header">
      <a className="brand" href="#top" aria-label="Carambola home"><img src={`${import.meta.env.BASE_URL}assets/carambola-icon.png`} alt="" /><span>carambola</span></a>
      <nav className="desktop-nav" aria-label="Main navigation"><a href="#features">{t.navFeatures}</a><a href="#how-it-works">{t.navHow}</a><a href="#privacy">{t.navPrivacy}</a></nav>
      <div className="nav-actions"><div className="language-switch" role="group" aria-label={t.ariaLanguage}><button type="button" aria-label="English" aria-pressed={language === "en"} onClick={() => setLanguage("en")}>EN</button><button type="button" aria-label="Türkçe" aria-pressed={language === "tr"} onClick={() => setLanguage("tr")}>TR</button></div><a className="nav-cta" href={RELEASES} target="_blank" rel="noreferrer">{t.navDownload}<Arrow diagonal /></a></div>
    </header>

    <main id="top">
      <section className="hero section-wrap">
        <div className="hero-copy"><div className="eyebrow"><span className="eyebrow-dot" />{t.eyebrow}</div><h1>{t.title}</h1><p className="hero-intro">{t.intro}</p><div className="hero-actions"><a className="button button-primary" href={RELEASES} target="_blank" rel="noreferrer">{t.download}<Arrow /></a><a className="button button-secondary" href={REPOSITORY} target="_blank" rel="noreferrer">{t.github}<Arrow diagonal /></a></div><div className="platform-note"><span className="platform-dots"><i /><i /><i /></span>{t.platform}</div></div>
        <div className="hero-art" aria-label={t.ariaMock}>
          <div className="art-orbit orbit-one" /><div className="art-orbit orbit-two" />
          <div className="app-window">
            <div className="window-bar"><span className="window-dots"><i /><i /><i /></span><span className="window-title">{t.preview}</span><span className="window-status"><b /> {t.ready}</span></div>
            <div className="capture-scene"><div className="scene-sky"><div className="scene-sun" /><div className="scene-hill hill-back" /><div className="scene-hill hill-front" /><div className="scene-path" /><div className="scene-tree tree-a"><i /><b /></div><div className="scene-tree tree-b"><i /><b /></div><div className="scene-cloud cloud-a" /><div className="scene-cloud cloud-b" /><span className="scene-label">14:32 <i /> {t.demoDay}</span></div>
              <div className="selection-box"><span className="crop crop-tl" /><span className="crop crop-tr" /><span className="crop crop-bl" /><span className="crop crop-br" /><span className="selection-note">{t.selection}</span><span className="annotation-arrow" /></div>
              <div className="dimension-pill">684 × 392</div>
            </div>
            <div className="floating-tools"><button className="tool-active" aria-label={t.toolSelect}><span className="cursor-icon">↖</span></button><button aria-label={t.toolPen}><span className="pen-icon">⌁</span></button><button aria-label={t.toolText}><span className="text-icon">T</span></button><button aria-label={t.toolMosaic}><span className="mosaic-icon"><i /><i /><i /><i /></span></button><span className="tool-divider" /><button className="tool-copy"><span className="copy-icon">▣</span>{t.toolCopy}</button></div>
          </div><div className="art-caption"><span className="caption-star">✳</span><span><b>{t.previewTitle}</b><small>{t.previewSub}</small></span><span className="caption-count">01 / 03</span></div>
        </div>
        <div className="hero-bottom"><span>01</span><span className="hero-bottom-line" /><span>{t.captureTagline}</span><span className="hero-bottom-end">{t.scrollExplore} ↓</span></div>
      </section>

      <section className="features-section section-wrap" id="features"><div className="section-heading"><div><div className="eyebrow section-eyebrow">{t.sectionLabel}</div><h2>{t.featureTitle}</h2></div><p>{t.featureIntro}</p></div><div className="feature-grid">{t.features.map(([n, title, description], i) => <article className="feature-item" key={n}><div className={`feature-symbol symbol-${i + 1}`} aria-hidden="true">{[<span className="symbol-spark">✳</span>, <span className="symbol-pen">↗</span>, <span className="symbol-mosaic"><i /><i /><i /><i /></span>, <span className="symbol-type">Aa</span>, <span className="symbol-copy">▣</span>, <span className="symbol-palette"><i /><i /><i /></span>][i]}</div><span className="feature-number">{n}</span><h3>{title}</h3><p>{description}</p></article>)}</div></section>

      <section className="workflow-section" id="how-it-works"><div className="section-wrap workflow-inner"><div className="eyebrow section-eyebrow">{t.workflowLabel}</div><h2>{t.workflowTitle}</h2><div className="steps-grid">{t.steps.map(([n, title, description], i) => <article className="step" key={n}><div className="step-top"><span className="step-number">{n}</span><span className="step-mark">{i === 0 ? "⌘" : i === 1 ? "↗" : "↳"}</span></div><h3>{title}</h3><p>{description}</p></article>)}</div></div></section>

      <section className="privacy-section section-wrap" id="privacy"><div className="privacy-visual" aria-hidden="true"><div className="privacy-ring ring-a" /><div className="privacy-ring ring-b" /><div className="privacy-core"><span>✳</span><small>{t.localByDesign}</small></div><span className="privacy-chip chip-one">PNG <b>↗</b></span><span className="privacy-chip chip-two">{t.yourDevice} <i /></span><span className="privacy-chip chip-three">{t.offlineFirst}</span></div><div className="privacy-copy"><div className="eyebrow section-eyebrow">{t.privacyLabel}</div><h2>{t.privacyTitle}</h2><p>{t.privacyBody}</p><div className="privacy-footnote"><span>✓</span>{t.privacyNote}</div></div></section>

      <section className="download-section" id="download"><div className="section-wrap download-inner"><div className="download-heading"><div className="eyebrow section-eyebrow">{t.downloadLabel}</div><h2>{t.downloadTitle}</h2><p>{t.downloadBody}</p><a href={RELEASES} target="_blank" rel="noreferrer" className="button button-primary download-button">{t.releases}<Arrow diagonal /></a><a className="source-link" href={REPOSITORY} target="_blank" rel="noreferrer">{t.source}<Arrow diagonal /></a></div><div className="download-info"><h3>{t.buildsTitle}</h3><div className="build-row"><span className="build-platform">⌘</span><div><b>{t.mac}</b><p>{t.macText}</p></div></div><div className="build-row"><span className="build-platform windows-mark">⊞</span><div><b>{t.windows}</b><p>{t.windowsText}</p></div></div><div className="build-row"><span className="build-platform linux-mark">◈</span><div><b>{t.linux}</b><p>{t.linuxText}</p></div></div></div></div></section>
    </main>
    <footer className="site-footer section-wrap"><a className="brand footer-brand" href="#top"><img src={`${import.meta.env.BASE_URL}assets/carambola-icon.png`} alt="" /><span>carambola</span></a><span className="footer-motto">{t.footerLine}</span><div className="footer-meta"><span>{t.license}</span><span>© {new Date().getFullYear()} Carambola</span></div><a className="footer-github" href={REPOSITORY} target="_blank" rel="noreferrer" aria-label="Carambola on GitHub">GitHub <Arrow diagonal /></a></footer>
  </>;
}

export default App;
