#!/bin/sh
# Prints the Homebrew cask for a release, for the tap (ahmedanjum30/homebrew-tap,
# Casks/b-downloader.rb). The release workflow runs it for each new version.
#
#   sh scripts/homebrew-cask.sh <version> <zip>   # sha256 from the zip
#   sh scripts/homebrew-cask.sh <version> <sha256>
#   URL=file:///path/to.zip sh scripts/homebrew-cask.sh ...   # local testing
#
# The app is signed ad hoc, not notarized, so Gatekeeper refuses it while it
# carries the quarantine flag Homebrew sets on downloads. The postflight
# removes the flag, which is why this lives in our own tap: the official
# homebrew/cask only takes notarized apps.

set -eu

version=$1
if [ -f "$2" ]; then
    sha256=$(shasum -a 256 "$2" | cut -d' ' -f1)
else
    sha256=$2
fi
url=${URL:-'https://github.com/ahmedanjum30/B-Downloader/releases/download/v#{version}/B-Downloader-#{version}-macos.zip'}

cat <<EOF
cask "b-downloader" do
  version "$version"
  sha256 "$sha256"

  url "$url"
  name "B-Downloader"
  desc "Download manager that fetches files over parallel connections"
  homepage "https://github.com/ahmedanjum30/B-Downloader"

  livecheck do
    url :url
    strategy :github_latest
  end

  depends_on arch: :arm64
  depends_on macos: ">= :big_sur"

  app "B-Downloader.app"

  # Signed ad hoc, not notarized: without this Gatekeeper won't open it.
  postflight do
    system_command "/usr/bin/xattr",
                   args: ["-dr", "com.apple.quarantine", "#{appdir}/B-Downloader.app"]
  end

  uninstall quit: "com.ahmadanjum.bdownloader"

  zap trash: [
    "~/Library/Application Support/B-Downloader",
    "~/Library/Application Support/BraveSoftware/Brave-Browser/NativeMessagingHosts/com.bdownloader.host.json",
    "~/Library/Application Support/Chromium/NativeMessagingHosts/com.bdownloader.host.json",
    "~/Library/Application Support/com.operasoftware.Opera/NativeMessagingHosts/com.bdownloader.host.json",
    "~/Library/Application Support/Google/Chrome/NativeMessagingHosts/com.bdownloader.host.json",
    "~/Library/Application Support/Microsoft Edge/NativeMessagingHosts/com.bdownloader.host.json",
    "~/Library/Application Support/Mozilla/NativeMessagingHosts/com.bdownloader.host.json",
    "~/Library/Application Support/Vivaldi/NativeMessagingHosts/com.bdownloader.host.json",
  ]
end
EOF
