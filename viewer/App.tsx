// The page `sonde share` writes: one HTML file holding this viewer and the run, which it is given as its
// parameters (`host.params`): nothing is fetched, so it opens from a disk as from a server.

import { host, mount } from "nsh";
import { Empty } from "../app/src/ui/kit";
import { t } from "../app/src/i18n";
import { type SharedRun, RunViewer } from "./RunViewer";

function App() {
  const data = host.params as unknown as SharedRun | undefined;
  return (
    <If when={data?.format === "sonde-share" ? data : null} else={<Empty icon="circle-alert" text={t("viewer.noRun")} />}>
      {(d) => <RunViewer data={d()} />}
    </If>
  );
}

mount(App);
