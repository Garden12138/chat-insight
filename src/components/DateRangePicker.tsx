import type { DateRange, RangePreset } from "../types";

interface DateRangePickerProps {
  preset: RangePreset;
  range: DateRange;
  onPresetChange: (preset: RangePreset) => void;
  onRangeChange: (range: DateRange) => void;
}

const presets: Array<{ value: RangePreset; label: string }> = [
  { value: "30d", label: "近 30 天" },
  { value: "1y", label: "近 1 年" },
  { value: "all", label: "全部" },
  { value: "custom", label: "自定义" },
];

export function DateRangePicker({ preset, range, onPresetChange, onRangeChange }: DateRangePickerProps) {
  return (
    <div className="range-picker" aria-label="时间范围">
      <div className="segmented">
        {presets.map((item) => (
          <button key={item.value} className={preset === item.value ? "active" : ""} onClick={() => onPresetChange(item.value)}>
            {item.label}
          </button>
        ))}
      </div>
      {preset === "custom" && (
        <div className="custom-range">
          <input aria-label="开始日期" type="date" value={range.from ?? ""} onChange={(event) => onRangeChange({ ...range, from: event.target.value || undefined })} />
          <span>至</span>
          <input aria-label="结束日期" type="date" value={range.to ?? ""} onChange={(event) => onRangeChange({ ...range, to: event.target.value || undefined })} />
        </div>
      )}
    </div>
  );
}
