// The viewer on a run bundled in: for tests and the dev server.

import { mount } from "nsh";
import sample from "./sample.json";
import { type SharedRun, RunViewer } from "./RunViewer";

mount(() => <RunViewer data={sample as unknown as SharedRun} />);
