const releaseLabel = document.getElementById("release-label");
const windowsBtn = document.getElementById("download-windows");
const macArmBtn = document.getElementById("download-mac-arm");
const macIntelBtn = document.getElementById("download-mac-intel");
const linuxX86Btn = document.getElementById("download-linux-x86");
const linuxArmBtn = document.getElementById("download-linux-arm");
const releaseApi = "https://api.github.com/repos/kpauly/feedie/releases/latest";

const labelLoading =
  releaseLabel?.dataset.loading ?? "Loading latest release...";
const labelPrefix = releaseLabel?.dataset.prefix ?? "Latest release";
const labelFallback =
  releaseLabel?.dataset.fallback ??
  "Latest release unavailable - see GitHub releases.";

if (releaseLabel) {
  releaseLabel.textContent = labelLoading;
}

async function loadRelease() {
  try {
    const response = await fetch(releaseApi, {
      headers: { Accept: "application/vnd.github+json" },
    });
    if (!response.ok) {
      throw new Error(`Status ${response.status}`);
    }
    const data = await response.json();
    const version = data.tag_name ?? "unknown";
    if (releaseLabel) {
      releaseLabel.textContent = `${labelPrefix}: ${version}`;
    }

    const assets = data.assets ?? [];
    const findAsset = (keyword) =>
      assets.find((asset) =>
        asset.name?.toLowerCase().includes(keyword.toLowerCase()),
      );

    const windowsAsset = findAsset("feediesetup");
    const macArm = findAsset("feedie-mac-arm64");
    const macIntel = findAsset("feedie-mac-intel");
    const linuxX86 = findAsset("linux-x86_64");
    const linuxArm = findAsset("linux-aarch64");

    if (windowsAsset?.browser_download_url && windowsBtn) {
      windowsBtn.href = windowsAsset.browser_download_url;
    }
    if (macArm?.browser_download_url && macArmBtn) {
      macArmBtn.href = macArm.browser_download_url;
    }
    if (macIntel?.browser_download_url && macIntelBtn) {
      macIntelBtn.href = macIntel.browser_download_url;
    }
    if (linuxX86?.browser_download_url && linuxX86Btn) {
      linuxX86Btn.href = linuxX86.browser_download_url;
    }
    if (linuxArm?.browser_download_url && linuxArmBtn) {
      linuxArmBtn.href = linuxArm.browser_download_url;
    }
  } catch (error) {
    if (releaseLabel) {
      releaseLabel.textContent = labelFallback;
    }
    console.warn("Release fetch failed", error);
  }
}

const year = document.getElementById("current-year");
if (year) {
  year.textContent = new Date().getFullYear().toString();
}

loadRelease();
