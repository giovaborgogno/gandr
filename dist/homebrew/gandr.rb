# Homebrew formula for gandr.
#
# To publish: create a tap repo `github.com/giovaborgogno/homebrew-tap`, drop this
# file in its `Formula/` directory, then after tagging a release fill in `sha256`
# with the output of:
#   curl -sL https://github.com/giovaborgogno/gandr/archive/refs/tags/v0.1.0.tar.gz | shasum -a 256
# Users then install with: `brew install giovaborgogno/tap/gandr`.
class Gandr < Formula
  desc "Read-only TUI to review code changes and browse your repo"
  homepage "https://github.com/giovaborgogno/gandr"
  url "https://github.com/giovaborgogno/gandr/archive/refs/tags/v0.1.0.tar.gz"
  sha256 "0000000000000000000000000000000000000000000000000000000000000000" # TODO: fill at release
  license "MIT"
  head "https://github.com/giovaborgogno/gandr.git", branch: "main"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args
  end

  test do
    # gandr is an interactive TUI (no --help/--version flag), so just verify the
    # binary installed and is executable.
    assert_predicate bin/"gandr", :executable?
  end
end
