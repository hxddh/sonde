// Toasts: what happened, briefly, over the app — "Copied", "Saved · Undo",
// "Run ended · 34 operations · Open", an error with its request. Built on
// nsh/base's ToastManager, ToastStack and Toast (after Native Shell's
// gallery); a terminal shows them on its bottom row.

import { computed, Overlay, reduceMotion, signal, Text, View } from "nsh";
import { Toast, type ToastEntry, ToastManager, ToastStack } from "nsh/base";
import { Icon, type IconName, type Tint } from "./icon";
import { terminal } from "./kit";

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
  /** Milliseconds it stays (4000; errors 8000). */
  duration?: number;
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
  for (const gone of advanced.removed) {
    const { [gone.id]: _, ...rest } = contents();
    contents.set(rest);
  }
  if (advanced.changed) sync();
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
  manager.push(id, id, { timeout: content.duration ?? (content.kind === "error" ? 8000 : 4000) });
  run();
  return id;
}

export function dismissToast(id: string) {
  manager.dismiss(id);
  run();
}

const KINDS: Record<ToastKind, { icon: IconName; tint: Tint; mark: string; text: string }> = {
  success: { icon: "circle-check", tint: "status-ok", mark: "✓", text: "font-semibold text-status-ok" },
  error: { icon: "circle-x", tint: "destructive", mark: "✗", text: "font-semibold text-destructive" },
  warning: { icon: "triangle-alert", tint: "warn", mark: "!", text: "font-semibold text-warn" },
  info: { icon: "info", tint: "info", mark: "i", text: "font-semibold text-info" },
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
        <If when={kind()}>{(k) => <Text class={KINDS[k()].text}>{KINDS[k()].mark}</Text>}</If>
        <Text class="font-semibold text-foreground line-clamp-1">{content().title}</Text>
        <If when={content().description}>
          <Text class="flex-1 text-muted-foreground line-clamp-1">{content().description}</Text>
        </If>
        <If when={content().action}>
          <Text role="button" class="font-semibold text-primary" onPress={act}>
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
      class={`w-[340px] flex-row items-start gap-2 rounded-(--radius-card) border border-border bg-popover px-3 py-3 shadow-lg ${reduceMotion() ? "" : "transition-all duration-200 ease-out"}`}
      startingClass="opacity-0 translate-y-3"
      endingClass="opacity-0 scale-95"
      swipingClass="transition-none"
    >
      <If when={kind()}>
        {(k) => (
          <View class="pt-px">
            <Icon name={KINDS[k()].icon} size={16} tint={KINDS[k()].tint} />
          </View>
        )}
      </If>
      <View class="min-w-0 flex-1 gap-0.5">
        <Text class="text-body font-medium text-popover-foreground">{content().title}</Text>
        <If when={content().description}>
          <Text class="text-small text-muted-foreground line-clamp-3">{content().description}</Text>
        </If>
      </View>
      <If when={content().action}>
        <View role="button" testID="toast-action" accessibilityLabel={content().action} onPress={act} class="-my-0.5 rounded-(--radius-sm) px-2 py-1 hover:bg-muted active:bg-accent">
          <Text class="text-small font-semibold text-primary">{content().action}</Text>
        </View>
      </If>
    </Toast>
  );
}

/** Where toasts show: the window's bottom right; a terminal's bottom row. */
export function Toaster() {
  const cancel = () => {
    const newest = shown().filter((e) => e.status !== "ending").at(-1);
    if (newest) dismissToast(newest.id);
  };
  const stack = (
    <ToastStack closeOnEscape={!terminal()} placement={terminal() ? "bottomCenter" : "bottomRight"} class={terminal() ? "w-full" : "w-[340px]"} onExpandedChange={(e) => expanded.set(e.expanded)} onCancel={cancel}>
      <Each of={shown()} key={(e: ToastEntry) => e.id}>
        {(entry) => <Item entry={entry()} />}
      </Each>
    </ToastStack>
  );
  if (terminal()) return stack;
  return <Overlay style={{ pointerEvents: "box-none", left: 0, right: 0, bottom: 0, padding: 16, alignItems: "flex-end" }}>{stack}</Overlay>;
}
