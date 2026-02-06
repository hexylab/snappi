interface Props {
  elapsed: number;
  isPaused: boolean;
  onStop: () => void;
  onPause: () => void;
}

export default function RecordingBar(props: Props) {
  const formatTime = (seconds: number) => {
    const m = Math.floor(seconds / 60);
    const s = seconds % 60;
    return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
  };

  return (
    <div class="fixed top-4 left-1/2 -translate-x-1/2 z-50">
      <div class="flex items-center gap-3 bg-slate-900/95 backdrop-blur-sm border border-slate-700/50 rounded-full px-4 py-2 shadow-2xl">
        <div class="flex items-center gap-2">
          <div class={`w-2.5 h-2.5 rounded-full ${props.isPaused ? "bg-yellow-400" : "bg-red-500 animate-pulse"}`} />
          <span class="text-xs font-medium text-slate-300">{props.isPaused ? "PAUSED" : "REC"}</span>
        </div>
        <span class="text-sm font-mono text-slate-200 min-w-[48px] text-center">{formatTime(props.elapsed)}</span>
        <div class="w-px h-4 bg-slate-700" />
        <button onClick={props.onPause} class="p-1.5 rounded-full hover:bg-slate-700/50 transition-colors text-slate-400 hover:text-white" title={props.isPaused ? "Resume" : "Pause"}>
          {props.isPaused ? (
            <svg class="w-4 h-4" viewBox="0 0 24 24" fill="currentColor"><polygon points="5 3 19 12 5 21 5 3" /></svg>
          ) : (
            <svg class="w-4 h-4" viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="4" width="4" height="16" /><rect x="14" y="4" width="4" height="16" /></svg>
          )}
        </button>
        <button onClick={props.onStop} class="p-1.5 rounded-full hover:bg-red-500/20 transition-colors text-red-400 hover:text-red-300" title="Stop Recording">
          <svg class="w-4 h-4" viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="6" width="12" height="12" rx="1" /></svg>
        </button>
      </div>
    </div>
  );
}
