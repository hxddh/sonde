// Toasts: what happened, briefly, over the app — "Copied", "Saved · Undo",
// "Run ended · 34 operations · Open", an error with its request. Built on
// nsh/base's ToastManager, ToastStack and Toast (after Native Shell's
// gallery); a terminal shows them on its bottom row.
//
// One stack for the app, shown once: a toast belongs to the window that has
// the keyboard focus when it is pushed (Native Shell's `focusedWindow()`),
// if that window has a Toaster; else to the main window. Each Toaster draws
// only its own window's (`currentWindow()`, read as it renders).

import { batch, cleanup, computed, Overlay, signal, Text, untracked, View } from "nsh";
import { Toast, type ToastEntry, ToastManager, ToastStack } from "nsh/base";
import { currentWindow, focusedWindow, isAvailable } from "nsh/desktop";
import { t } from "../i18n";
import { Icon, type IconName } from "./icon";
import { Button, EMPHASIS, glass, IconButton, POPUP, Strong, terminal, SURFACE, TEXT, TONE, type Tone, TOAST_MOTION } from "./kit";

export type ToastKind = "success" | "error" | "warning" | "info";

export interface ToastContent {
  /** Showing another with the same id replaces it. */
  id?: string;
  title: string;
  description?: string;
  kind?: ToastKind;
  /** A button that closes it and calls `onAction`. */
  action?: string;
  onAction?: () => void;
  /** Milliseconds it stays (4000). An error stays until it is closed or
   * replaced (a terminal, which has no close button, keeps it 8000). */
  duration?: number;
}

/** The main window's id, and the windows that have a Toaster. */
let mainWindow = "main";
const stacks = new Map<string, number>();
/** The window each toast shows in. */
const windowOf = signal<Record<string, string>>({});

/** Where a toast pushed now shows: the focused window when it can show one, else the main window. */
function target(): string {
  if (!isAvailable) return mainWindow;
  try {
    const focused = untracked(() => focusedWindow());
    if (focused && stacks.has(focused.id)) return focused.id;
  } catch {
    // no windows to ask (a terminal, the web)
  }
  return mainWindow;
}

const LIMIT = 3;
const TICK = 50;
const manager = new ToastManager(terminal() ? { exitDuration: 0 } : undefined);
const shown = signal<ToastEntry[]>([]);
const expanded = signal(false);
const contents = signal<Record<string, ToastContent>>({});
let ticking: ReturnType<typeof setInterval> | undefined;
let count = 0;

function sync() {
  shown.set(manager.visible(LIMIT));
}

function tick() {
  const advanced = manager.advance(expanded());
  // at once: a toast that is gone leaves the list with its window. One by one, a toast of
  // another window was the main window's for a moment (no window: the main one), and two
  // going together drew rows there that were never shown (Native Shell: "a kept row is gone")
  batch(() => {
    if (advanced.changed) sync();
    for (const gone of advanced.removed) {
      const { [gone.id]: _, ...rest } = contents();
      contents.set(rest);
      const { [gone.id]: __, ...others } = windowOf();
      windowOf.set(others);
    }
  });
  if (manager.isEmpty() && ticking !== undefined) {
    clearInterval(ticking);
    ticking = undefined;
  }
}

function run() {
  sync();
  ticking ??= setInterval(tick, TICK);
}

/** Shows a toast; returns its id. */
export function toast(content: ToastContent): string {
  const id = content.id ?? `toast-${++count}`;
  if (terminal()) for (const e of shown()) if (e.id !== id) manager.dismiss(e.id);
  contents.set({ ...contents(), [id]: content });
  windowOf.set({ ...untracked(windowOf), [id]: target() });
  const timeout = content.duration ?? (content.kind !== "error" ? 4000 : terminal() ? 8000 : undefined);
  manager.push(id, id, timeout === undefined ? {} : { timeout });
  run();
  return id;
}

export function dismissToast(id: string) {
  manager.dismiss(id);
  run();
}

/** Each kind's icon, its mark in a terminal, and its tone in the kit's one table (`TONE`). */
const KINDS: Record<ToastKind, { icon: IconName; mark: string; tone: Tone }> = {
  success: { icon: "circle-check", mark: "✓", tone: "ok" },
  error: { icon: "circle-x", mark: "✗", tone: "fail" },
  warning: { icon: "triangle-alert", mark: "!", tone: "warn" },
  info: { icon: "info", mark: "i", tone: "info" },
};

function Item(props: { entry: ToastEntry }) {
  const content = () => contents()[props.entry.id] ?? { title: "" };
  const kind = computed(() => content().kind);
  const act = () => {
    content().onAction?.();
    dismissToast(props.entry.id);
  };
  if (terminal()) {
    return (
      <Toast
        transitionStatus={props.entry.status}
        accessibilityLabel={[content().title, content().description].filter(Boolean).join(". ")}
        onDismissRequest={() => dismissToast(props.entry.id)}
        swipeDirection={null}
        class="flex-1 flex-row items-center gap-1 px-1"
      >
        <If when={kind()}>{(k) => <Text class={TONE[KINDS[k()].tone].text}>{KINDS[k()].mark}</Text>}</If>
        <Strong class="text-foreground line-clamp-1">{content().title}</Strong>
        <If when={content().description}>
          <Text class="flex-1 text-muted-foreground line-clamp-1">{content().description}</Text>
        </If>
        <If when={content().action}>
          {/* emphasis: the one thing a terminal's toast lets be pressed */}
          <Text role="button" class={`${EMPHASIS} text-primary`} onPress={act}>
            {content().action}
          </Text>
        </If>
      </Toast>
    );
  }
  return (
    <Toast
      testID="toast"
      transitionStatus={props.entry.status}
      accessibilityLabel={[content().title, content().description].filter(Boolean).join(". ")}
      onDismissRequest={() => dismissToast(props.entry.id)}
      class={`${POPUP} ${SURFACE.toast} flex-row items-start gap-2 px-3 py-3 ${TOAST_MOTION}`}
      style={glass}
      startingClass="opacity-0 translate-y-3"
      endingClass="opacity-0 scale-95"
      swipingClass="transition-none"
    >
      <If when={kind()}>
        {(k) => (
          <View class="pt-px">
            <Icon name={KINDS[k()].icon} tint={TONE[KINDS[k()].tone].icon} />
          </View>
        )}
      </If>
      <View class="min-w-0 flex-1 gap-1">
        {/* its title regular, on one line: the icon says what kind it is */}
        <Text class="text-body text-popover-foreground line-clamp-1">{content().title}</Text>
        <If when={content().description}>
          <Text class={`${TEXT.note} line-clamp-3`}>{content().description}</Text>
        </If>
      </View>
      {/* what can be done about it, and Close: real buttons, with the keyboard's ring */}
      <View class="-my-1 shrink-0 flex-row items-center gap-1">
        <If when={content().action}>{(a) => <Button small variant="outline" label={a()} onPress={act} testID="toast-action" />}</If>
        <IconButton icon="x" label={t("common.close")} onPress={() => dismissToast(props.entry.id)} testID="toast-close" />
      </View>
    </Toast>
  );
}

/** Where toasts show: the window's bottom right; a terminal's bottom row.
 * Escape is not theirs (it belongs to the sheet, dialog or palette that is
 * open): every toast has its Close. */
export function Toaster() {
  // this Toaster's window, read while rendering
  let own = mainWindow;
  if (isAvailable) {
    try {
      const w = currentWindow();
      own = w.id;
      if (w.main) mainWindow = own;
    } catch {
      // no windows here
    }
  }
  stacks.set(own, (stacks.get(own) ?? 0) + 1);
  cleanup(() => {
    const left = (stacks.get(own) ?? 1) - 1;
    if (left > 0) return void stacks.set(own, left);
    stacks.delete(own);
    // the window is gone: what it was showing goes with it
    for (const [id, w] of Object.entries(untracked(windowOf))) if (w === own && own !== mainWindow) dismissToast(id);
  });
  const mine = () => shown().filter((e) => (windowOf()[e.id] ?? mainWindow) === own);
  const stack = (
    <ToastStack closeOnEscape={false} placement={terminal() ? "bottomCenter" : "bottomRight"} class={terminal() ? "w-full" : SURFACE.toast} onExpandedChange={(e) => expanded.set(e.expanded)}>
      <Each of={mine()} key={(e: ToastEntry) => e.id}>
        {(entry) => <Item entry={entry()} />}
      </Each>
    </ToastStack>
  );
  if (terminal()) return stack;
  return <Overlay style={{ pointerEvents: "box-none", left: 0, right: 0, bottom: 0, padding: 16, alignItems: "flex-end" }}>{stack}</Overlay>;
}
