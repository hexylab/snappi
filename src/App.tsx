import { createSignal, Match, onCleanup, onMount, Switch } from "solid-js";
import { listen } from "@tauri-apps/api/event";
import Home from "./pages/Home";
import Preview from "./pages/Preview";
import RecordingBar, { type BarPhase } from "./components/RecordingBar";
import {
  getRecordingState,
  pauseRecording,
  resumeRecording,
  setRecordingBar,
  startRecording,
  stopRecording,
} from "./lib/commands";

type Page = "home" | "preview";

function App() {
  const [page, setPage] = createSignal<Page>("home");
  const [recordingId, setRecordingId] = createSignal<string | null>(null);
  /** 録画中ならバーの状態。null なら録画していない */
  const [bar, setBar] = createSignal<BarPhase | null>(null);
  const [settingsOpen, setSettingsOpen] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  // 経過時間（一時停止中は進めない）
  const [elapsedMs, setElapsedMs] = createSignal(0);
  let accumulatedMs = 0;
  let runningSince: number | null = null;
  let timer: number | undefined;
  const tick = () => setElapsedMs(accumulatedMs + (runningSince === null ? 0 : performance.now() - runningSince));
  const startClock = () => {
    runningSince = performance.now();
    clearInterval(timer);
    timer = window.setInterval(tick, 250);
  };
  const stopClock = () => {
    if (runningSince !== null) accumulatedMs += performance.now() - runningSince;
    runningSince = null;
    clearInterval(timer);
    tick();
  };
  onCleanup(() => clearInterval(timer));

  /** 開始・停止の処理中は次の操作を受け付けない（ショートカットの連打など） */
  let busy = false;

  const start = async () => {
    setError(null);
    setSettingsOpen(false);
    try {
      // 録画に映らないよう、先にバーにしてから録画を始める
      await setRecordingBar(true);
      await startRecording();
      accumulatedMs = 0;
      setElapsedMs(0);
      startClock();
      setBar("recording");
    } catch (e) {
      console.error("start_recording failed:", e);
      await setRecordingBar(false).catch(() => {});
      setBar(null);
      setError(`録画を開始できませんでした: ${e}`);
    }
  };

  const stop = async () => {
    stopClock();
    setBar("finishing");
    let id: string | null = null;
    try {
      id = await stopRecording();
    } catch (e) {
      console.error("stop_recording failed:", e);
      setError(`録画を保存できませんでした: ${e}`);
    }
    await setRecordingBar(false).catch((e) => console.error("set_recording_bar failed:", e));
    if (id) {
      setRecordingId(id);
      setPage("preview");
    } else {
      setPage("home");
    }
    setBar(null);
  };

  const toggleRecording = async () => {
    if (busy) return;
    busy = true;
    try {
      const phase = bar();
      if (phase === null) await start();
      else if (phase !== "finishing") await stop();
    } finally {
      busy = false;
    }
  };

  const togglePause = async () => {
    try {
      if (bar() === "recording") {
        await pauseRecording();
        stopClock();
        setBar("paused");
      } else if (bar() === "paused") {
        await resumeRecording();
        startClock();
        setBar("recording");
      }
    } catch (e) {
      console.error("pause/resume failed:", e);
    }
  };

  onMount(async () => {
    // 前回の録画が残っていたら（画面の再読み込みなど）バーの表示に合わせる
    const state = await getRecordingState();
    if (state === "Recording" || state === "Paused") {
      await setRecordingBar(true).catch(() => {});
      setBar(state === "Recording" ? "recording" : "paused");
      if (state === "Recording") startClock();
    }

    const unlisten = await Promise.all([
      listen("tray-start-recording", () => void toggleRecording()),
      listen("shortcut-toggle-recording", () => void toggleRecording()),
      listen("tray-open-settings", () => {
        if (bar() !== null) return;
        setPage("home");
        setSettingsOpen(true);
      }),
    ]);
    onCleanup(() => unlisten.forEach((u) => u()));
  });

  return (
    <Switch>
      <Match when={bar()}>
        {(phase) => (
          <RecordingBar
            phase={phase()}
            elapsedMs={elapsedMs()}
            onPauseToggle={togglePause}
            onStop={() => void toggleRecording()}
          />
        )}
      </Match>
      <Match when={page() === "preview" && recordingId()}>
        {(id) => <Preview recordingId={id()} onBack={() => setPage("home")} />}
      </Match>
      <Match when={true}>
        <Home
          onStartRecording={() => void toggleRecording()}
          onOpenPreview={(id) => {
            setRecordingId(id);
            setPage("preview");
          }}
          settingsOpen={settingsOpen()}
          onSettingsOpenChange={setSettingsOpen}
          error={error()}
          onDismissError={() => setError(null)}
        />
      </Match>
    </Switch>
  );
}

export default App;
