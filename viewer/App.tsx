// The page `sonde share` publishes: reads the session beside it.

import { mount, resource } from "nsh";
import { Loading, type SharedSession, SessionViewer } from "./SessionViewer";

function App() {
  const data = resource(async () => {
    const res = await fetch("session.json");
    if (!res.ok) throw new Error(`session.json: HTTP ${res.status}`);
    return (await res.json()) as SharedSession;
  });
  return (
    <If when={data()} else={<Loading error={data.error ? String(data.error) : null} />}>
      {(d) => <SessionViewer data={d()} />}
    </If>
  );
}

mount(App);
