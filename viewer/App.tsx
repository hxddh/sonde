// The page `sonde share` publishes: reads the session beside it.

import { mount, resource } from "nsh";
import { Await } from "../app/src/ui/kit";
import { type SharedSession, SessionViewer } from "./SessionViewer";

function App() {
  const data = resource(async () => {
    const res = await fetch("session.json");
    if (!res.ok) throw new Error(`session.json: HTTP ${res.status}`);
    return (await res.json()) as SharedSession;
  });
  return (
    // loading, failed (why, and Try again) or there: `error` and `loading` are the resource's readers, called
    <Await loading={data.loading()} error={data.error()} retry={() => data.refetch()}>
      <If when={data()}>{(d) => <SessionViewer data={d()} />}</If>
    </Await>
  );
}

mount(App);
