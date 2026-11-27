import SwiftUI

// iOS 16.4 support (Kamal's own build, 2026-11-27): the iOS 17 API where it runs, otherwise the same behaviour
// built from iOS 14's `onChange(of:perform:)` and `onAppear`.

extension View {
    /// `onChange(of:initial:_:)` from iOS 17, on iOS 16 too: the action gets the old and the new value, and
    /// with `initial` runs once as the view appears (old and new the same), as iOS 17's does. On iOS 16 the
    /// action called is always the newest the view handed in, so what it captured is this update's (iOS 16's own
    /// `onChange` runs the closure it was given an update earlier), and the old value is kept here.
    func onChangeCompat<V: Equatable>(of value: V, initial: Bool = false,
                                      _ action: @escaping (_ oldValue: V, _ newValue: V) -> Void) -> some View {
        modifier(OnChangeCompat(value: value, initial: initial, action: action))
    }
}

private struct OnChangeCompat<V: Equatable>: ViewModifier {
    let value: V
    let initial: Bool
    let action: (V, V) -> Void
    /// iOS 16 only: the value last seen and the newest action.
    @State private var box = OnChangeBox<V>()

    @ViewBuilder func body(content: Content) -> some View {
        if #available(iOS 17, *) {
            content.onChange(of: value, initial: initial, action)
        } else {
            let _ = box.action = action
            content
                .onAppear {
                    box.old = value
                    if initial { action(value, value) }
                }
                .onChange(of: value) { new in
                    let old = box.old ?? new
                    box.old = new
                    box.action?(old, new)
                }
        }
    }
}

private final class OnChangeBox<V> {
    var old: V?
    var action: ((V, V) -> Void)?
}

// MARK: - iOS 17 modifiers, nothing or their iOS 16 equivalent before it

extension View {
    /// `sensoryFeedback(.warning, trigger:)`; on iOS 16 the same haptic from UIKit (a no-op on an iPad).
    func warningFeedback<T: Equatable>(trigger: T) -> some View {
        modifier(Feedback16(trigger: trigger, kind: .warning))
    }

    /// `sensoryFeedback(.selection, trigger:)`; on iOS 16 the same tick from UIKit.
    func selectionFeedback<T: Equatable>(trigger: T) -> some View {
        modifier(Feedback16(trigger: trigger, kind: .selection))
    }

    /// `scrollIndicatorsFlash(onAppear: true)`; nothing on iOS 16.
    func flashesScrollIndicatorsOnAppear() -> some View { modifier(FlashOnAppear()) }

    /// `scrollIndicatorsFlash(trigger:)`; nothing on iOS 16.
    func flashesScrollIndicators<T: Equatable>(trigger: T) -> some View { modifier(FlashOnTrigger(trigger: trigger)) }

    /// A scroll view's `contentMargins(edges, insets, for: .scrollContent)` on iOS 17; on iOS 16 nothing here,
    /// and its content takes the same insets as padding (`paddingBefore17`).
    func scrollContentMargins(_ edges: Edge.Set, _ insets: EdgeInsets) -> some View {
        modifier(ContentMargins17(edges: edges, insets: insets))
    }

    /// The content inside a scroll view that has `scrollContentMargins`: on iOS 16 only, these insets as padding.
    func paddingBefore17(_ insets: EdgeInsets) -> some View { modifier(Padding16(insets: insets)) }
}

extension Animation {
    /// `.spring(duration:bounce:)` on iOS 17; on iOS 16 the same spring by its older terms (a response of
    /// `duration`, a damping fraction of 1 − bounce, which is how iOS 17 maps the two).
    static func springCompat(duration: Double, bounce: Double = 0) -> Animation {
        if #available(iOS 17, *) { return .spring(duration: duration, bounce: bounce) }
        return .spring(response: duration, dampingFraction: 1 - bounce)
    }
}

private enum FeedbackKind { case warning, selection }

private struct Feedback16<T: Equatable>: ViewModifier {
    let trigger: T
    let kind: FeedbackKind

    @ViewBuilder func body(content: Content) -> some View {
        if #available(iOS 17, *) {
            content.sensoryFeedback(kind == .warning ? .warning : .selection, trigger: trigger)
        } else {
            content.onChangeCompat(of: trigger) { _, _ in
                switch kind {
                case .warning: UINotificationFeedbackGenerator().notificationOccurred(.warning)
                case .selection: UISelectionFeedbackGenerator().selectionChanged()
                }
            }
        }
    }
}

private struct FlashOnAppear: ViewModifier {
    @ViewBuilder func body(content: Content) -> some View {
        if #available(iOS 17, *) { content.scrollIndicatorsFlash(onAppear: true) } else { content }
    }
}

private struct FlashOnTrigger<T: Equatable>: ViewModifier {
    let trigger: T
    @ViewBuilder func body(content: Content) -> some View {
        if #available(iOS 17, *) { content.scrollIndicatorsFlash(trigger: trigger) } else { content }
    }
}

private struct ContentMargins17: ViewModifier {
    let edges: Edge.Set
    let insets: EdgeInsets
    @ViewBuilder func body(content: Content) -> some View {
        if #available(iOS 17, *) { content.contentMargins(edges, insets, for: .scrollContent) } else { content }
    }
}

private struct Padding16: ViewModifier {
    let insets: EdgeInsets
    @ViewBuilder func body(content: Content) -> some View {
        if #available(iOS 17, *) { content } else { content.padding(insets) }
    }
}

extension View {
    /// `scrollTargetLayout()`; nothing on iOS 16.
    func scrollTargetLayoutIf17() -> some View { modifier(TargetLayout17()) }
    /// `scrollTargetBehavior(.viewAligned)`; on iOS 16 the scroll stops where it stops.
    func viewAlignedScrollIf17() -> some View { modifier(ViewAligned17()) }
    /// `safeAreaPadding(insets)`; on iOS 16 plain padding.
    func safeAreaPaddingCompat(_ insets: EdgeInsets) -> some View { modifier(SafeAreaPadding16(insets: insets)) }
}

private struct TargetLayout17: ViewModifier {
    @ViewBuilder func body(content: Content) -> some View {
        if #available(iOS 17, *) { content.scrollTargetLayout() } else { content }
    }
}

private struct ViewAligned17: ViewModifier {
    @ViewBuilder func body(content: Content) -> some View {
        if #available(iOS 17, *) { content.scrollTargetBehavior(.viewAligned) } else { content }
    }
}

private struct SafeAreaPadding16: ViewModifier {
    let insets: EdgeInsets
    @ViewBuilder func body(content: Content) -> some View {
        if #available(iOS 17, *) { content.safeAreaPadding(insets) } else { content.padding(insets) }
    }
}
