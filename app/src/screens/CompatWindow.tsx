// Compatibility in a window of its own: probes run on the chosen endpoints,
// the matrix of what each one does differently from AWS, saved runs and
// their difference. Opened from Settings › Endpoints; a terminal keeps it
// with the buckets.

import { View } from "nsh";
import { Window } from "nsh/desktop";
import { t } from "../i18n";
import { FocusRest } from "../ui/kit";
import { Toaster } from "../ui/toast";
import { Probes } from "./Probes";

export function openCompatWindow() {
  const open = Window.get("compat");
  if (open) {
    open.show();
    open.focus();
    return;
  }
  Window.open(
    () => (
      <View class="flex-1 bg-background" testID="compat-window">
        <FocusRest />
        <Probes />
        <Toaster />
      </View>
    ),
    { id: "compat", title: t("compat.title"), defaultSize: { width: 1040, height: 720 }, minSize: { width: 720, height: 480 }, restoreBounds: true },
  );
}
