import { createSignal, onMount, Show } from "solid-js";
import { listen } from "@tauri-apps/api/event";
import RecordingList from "./pages/RecordingList";
import Preview from "./pages/Preview";
import Settings from "./pages/Settings";
import RecordingBar from "./components/RecordingBar";
import {
  startRecording,
  stopRecording,
  getRecordingState,
} from "./lib/commands";
import type { RecordingState } from "./lib/types";

type Page = "list" | "preview" | "settings";

function App() {
  const [page, setPage] = createSignal<Page>("list");
  const [recordingState, setRecordingState] =
    createSignal<RecordingState>("Idle");
  const [currentRecordingId, setCurrentRecordingId] = createSignal<
    string | null
  >(null);
  const [elapsed, setElapsed] = createSignal(0);
  let timerRef: number | undefined;

  onMount(async () => {
    const state = await getRecordingState();
    setRecordingState(state);

    await listen("tray-start-recording", () => handleToggleRecording());
    await listen("tray-open-settings", () => setPage("settings"));
    await listen("shortcut-toggle-recording", () => handleToggleRecording());
  });

  const handleToggleRecording = async () => {
    const state = recordingState();
    if (state === "Idle") {
      try {
        await startRecording();
        setRecordingState("Recording");
        setElapsed(0);
        timerRef = window.setInterval(
          () => setElapsed((e) => e + 1),
          1000
        );
      } catch (e) {
        console.error("Failed to start recording:", e);
      }
    } else if (state === "Recording" || state === "Paused") {
      if (timerRef) clearInterval(timerRef);
      try {
        const id = await stopRecording();
        setRecordingState("Idle");
        setCurrentRecordingId(id);
        setPage("preview");
      } catch (e) {
        console.error("Failed to stop recording:", e);
        // Reset state so UI doesn't get stuck in Recording mode
        setRecordingState("Idle");
      }
    }
  };

  return (
    <div class="min-h-screen bg-slate-900 text-slate-200">
      <Show
        when={
          recordingState() === "Recording" || recordingState() === "Paused"
        }
      >
        <RecordingBar
          elapsed={elapsed()}
          isPaused={recordingState() === "Paused"}
          onStop={handleToggleRecording}
          onPause={async () => {
            const { pauseRecording, resumeRecording } = await import(
              "./lib/commands"
            );
            if (recordingState() === "Recording") {
              await pauseRecording();
              setRecordingState("Paused");
              if (timerRef) clearInterval(timerRef);
            } else {
              await resumeRecording();
              setRecordingState("Recording");
              timerRef = window.setInterval(
                () => setElapsed((e) => e + 1),
                1000
              );
            }
          }}
        />
      </Show>

      <Show when={page() === "list"}>
        <RecordingList
          onStartRecording={handleToggleRecording}
          onOpenSettings={() => setPage("settings")}
          onOpenPreview={(id) => {
            setCurrentRecordingId(id);
            setPage("preview");
          }}
          recordingState={recordingState()}
        />
      </Show>

      <Show when={page() === "preview"}>
        <Preview
          recordingId={currentRecordingId()}
          onClose={() => {
            setCurrentRecordingId(null);
            setPage("list");
          }}
          onRedo={handleToggleRecording}
        />
      </Show>

      <Show when={page() === "settings"}>
        <Settings onClose={() => setPage("list")} />
      </Show>
    </div>
  );
}

export default App;
