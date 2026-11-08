import SwiftUI
import UIKit

/// The game unpacking an archive it found in its folder (or the launcher downloaded): what it is doing,
/// how far along, and what went wrong. Shown over the launcher while `GameLink.importing` says so; the
/// game keeps the work on its own thread, so the screen stays live.
struct ImportingOverlay: View {
    let progress: ImportProgress
    var dismiss: () -> Void

    var body: some View {
        ZStack {
            Theme.background.ignoresSafeArea()
            VStack(alignment: .leading, spacing: 22) {
                Spacer()
                VStack(alignment: .leading, spacing: 8) {
                    Text("HORIZON")
                        .font(Theme.pixel(14, .bold))
                        .tracking(2)
                        .foregroundStyle(Theme.accentText)
                    Text(failed ? "Could not install the files" : "Preparing game files")
                        .font(Theme.pixel(28, .bold))
                        .foregroundStyle(Theme.text)
                    Text(subtitle)
                        .font(Theme.sans(15))
                        .foregroundStyle(Theme.textMuted)
                        .fixedSize(horizontal: false, vertical: true)
                }
                HStack(spacing: 14) {
                    if progress.totalFiles > 0 {
                        ProgressView(value: progress.fraction)
                            .progressViewStyle(.linear)
                            .tint(failed ? Theme.danger : Theme.accent)
                            .scaleEffect(x: 1, y: 2, anchor: .center)
                            .animation(.easeOut(duration: 0.25), value: progress.fraction)
                    } else {
                        ProgressView()
                            .progressViewStyle(.linear)
                            .tint(failed ? Theme.danger : Theme.accent)
                            .scaleEffect(x: 1, y: 2, anchor: .center)
                    }
                    if failed {
                        Image(systemName: "exclamationmark.triangle.fill")
                            .foregroundStyle(Theme.danger)
                    } else {
                        ProgressView()
                            .progressViewStyle(.circular)
                            .tint(Theme.accentText)
                    }
                }
                .frame(minHeight: 28)
                stats
                if failed {
                    PrimaryButton(title: "Close", icon: "xmark") {
                        dismiss()
                    }
                }
                Spacer()
                Text(footnote)
                    .font(Theme.sans(12))
                    .foregroundStyle(Theme.textFaint)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .padding(.horizontal, 28)
            .padding(.vertical, 24)
        }
        .onAppear { UIApplication.shared.isIdleTimerDisabled = true }
        .onDisappear { UIApplication.shared.isIdleTimerDisabled = false }
        .accessibilityIdentifier("launcher-importing")
    }

    private var failed: Bool { progress.error != nil }

    private var subtitle: String {
        if let error = progress.error {
            return error
        }
        let name = progress.label.isEmpty ? "the game files" : progress.label
        return "Horizon is unpacking \(name) into its folder. This happens once, and the game loads as soon as it is done."
    }

    private var footnote: String {
        if failed {
            return "The archive is left in place: check that it is complete (a full download), then drop it in again."
        }
        return "Keep Horizon open while this runs: the screen stays on, and if it is interrupted the unpacking starts over next time."
    }

    private var stats: some View {
        VStack(spacing: 0) {
            row("Files", progress.totalFiles > 0
                ? "\(progress.files) of \(progress.totalFiles)"
                : (progress.files > 0 ? "\(progress.files)" : "–"))
            Divider().overlay(Theme.border)
            row("Unpacked", progress.bytes > 0 ? ByteCountFormatter.string(fromByteCount: progress.bytes, countStyle: .file) : "–")
        }
        .background(Theme.surface, in: RoundedRectangle(cornerRadius: 10))
        .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(Theme.border))
    }

    private func row(_ label: String, _ value: String) -> some View {
        HStack {
            Text(label)
                .font(Theme.sans(14))
                .foregroundStyle(Theme.textMuted)
            Spacer()
            Text(value)
                .font(Theme.mono(14))
                .foregroundStyle(Theme.text)
                .monospacedDigit()
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
    }
}
