#!/usr/bin/env bun
// Writes the cask for one release. Publish it at Casks/minutae.rb in
// CinematicGenius007/homebrew-tap after publishing the matching GitHub Release.
//
//   bun scripts/homebrew-cask.ts <version> <arm64-dmg-sha256> <intel-dmg-sha256> <out.rb>
import { writeFileSync } from "node:fs";

const REPO = "CinematicGenius007/minutae";
const [version, armHash, intelHash, out] = process.argv.slice(2);
const sha256 = /^[0-9a-f]{64}$/i;
if (!version || !/^\d+\.\d+\.\d+(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?$/.test(version)
    || !sha256.test(armHash ?? "") || !sha256.test(intelHash ?? "") || !out) {
  throw new Error("usage: bun scripts/homebrew-cask.ts <version> <arm64-dmg-sha256> <intel-dmg-sha256> <out.rb>");
}

const cask = `cask "minutae" do
  arch arm: "aarch64", intel: "x64"

  version "${version}"
  sha256 arm:   "${armHash.toLowerCase()}",
         intel: "${intelHash.toLowerCase()}"

  url "https://github.com/${REPO}/releases/download/v#{version}/Minutae_#{version}_#{arch}.dmg"
  name "Minutae"
  desc "Local-first meeting notes and dictation"
  homepage "https://github.com/${REPO}"

  livecheck do
    url :url
    strategy :github_latest
  end

  depends_on macos: :ventura

  app "Minutae.app"

  uninstall quit: "com.nebula.minutae"

  # Leave ~/Library/Minutae intact, including recordings and transcripts.
  zap trash: [
    "~/Library/Caches/com.nebula.minutae",
    "~/Library/Saved Application State/com.nebula.minutae.savedState",
  ]

  caveats <<~EOS
    Minutae is not notarized. After the first blocked launch, go to
    System Settings > Privacy & Security > Open Anyway.
    Homebrew 7 carries this approval forward on upgrades signed with the same identity.
    Update with: brew upgrade --cask minutae
  EOS
end
`;

writeFileSync(out, cask);
console.log(`wrote ${out} for ${version}`);
