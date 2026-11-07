// The viewer on a session bundled in: for tests and the dev server.

import { mount } from "nsh";
import sample from "./sample.json";
import { type SharedSession, SessionViewer } from "./SessionViewer";

mount(() => <SessionViewer data={sample as unknown as SharedSession} />);
