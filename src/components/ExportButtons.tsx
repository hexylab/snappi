import type { ExportFormat } from "../lib/types";

interface Props {
  onExport: (format: ExportFormat) => void;
  exporting: boolean;
}

export default function ExportButtons(props: Props) {
  return (
    <div class="flex gap-3">
      <button
        onClick={() => props.onExport("Mp4")}
        disabled={props.exporting}
        class="flex-1 py-2.5 px-4 rounded-xl font-medium transition-all bg-gradient-to-r from-purple-500 to-blue-500 hover:from-purple-600 hover:to-blue-600 text-white shadow-lg shadow-purple-500/20 disabled:opacity-50 disabled:cursor-not-allowed text-sm"
      >
        {props.exporting ? "Exporting..." : "Export MP4"}
      </button>
      <button
        onClick={() => props.onExport("Gif")}
        disabled={props.exporting}
        class="py-2.5 px-4 rounded-xl font-medium transition-all border border-slate-700 text-slate-300 hover:bg-slate-800 disabled:opacity-50 disabled:cursor-not-allowed text-sm"
      >
        GIF
      </button>
      <button
        onClick={() => props.onExport("WebM")}
        disabled={props.exporting}
        class="py-2.5 px-4 rounded-xl font-medium transition-all border border-slate-700 text-slate-300 hover:bg-slate-800 disabled:opacity-50 disabled:cursor-not-allowed text-sm"
      >
        WebM
      </button>
    </div>
  );
}
