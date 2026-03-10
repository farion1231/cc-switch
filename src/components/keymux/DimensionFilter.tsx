import { Checkbox } from "@/components/ui/checkbox";
import { Label } from "@/components/ui/label";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import type { Dimension } from "./types";

interface DimensionFilterProps {
  dimensions: Dimension[];
  onChange: (dimensions: Dimension[]) => void;
}

export function DimensionFilter({
  dimensions,
  onChange,
}: DimensionFilterProps) {
  const toggleValue = (dimIndex: number, value: string) => {
    const newDimensions = [...dimensions];
    const dim = newDimensions[dimIndex];
    const selected = dim.selected || dim.values;
    const newSelected = selected.includes(value)
      ? selected.filter((v) => v !== value)
      : [...selected, value];
    newDimensions[dimIndex] = { ...dim, selected: newSelected };
    onChange(newDimensions);
  };

  const selectAll = (dimIndex: number) => {
    const newDimensions = [...dimensions];
    newDimensions[dimIndex] = {
      ...newDimensions[dimIndex],
      selected: newDimensions[dimIndex].values,
    };
    onChange(newDimensions);
  };

  const clearAll = (dimIndex: number) => {
    const newDimensions = [...dimensions];
    newDimensions[dimIndex] = {
      ...newDimensions[dimIndex],
      selected: [],
    };
    onChange(newDimensions);
  };

  return (
    <div className="space-y-4">
      {dimensions.map((dim, dimIndex) => (
        <Card key={dim.name}>
          <CardHeader className="pb-2">
            <div className="flex items-center justify-between">
              <CardTitle className="text-sm">{dim.name}</CardTitle>
              <div className="flex gap-1">
                <button
                  className="text-xs text-muted-foreground hover:text-foreground"
                  onClick={() => selectAll(dimIndex)}
                >
                  All
                </button>
                <span className="text-muted-foreground">|</span>
                <button
                  className="text-xs text-muted-foreground hover:text-foreground"
                  onClick={() => clearAll(dimIndex)}
                >
                  None
                </button>
              </div>
            </div>
          </CardHeader>
          <CardContent className="space-y-2">
            {dim.values.map((value) => {
              const isSelected = (dim.selected || dim.values).includes(value);
              return (
                <div key={value} className="flex items-center gap-2">
                  <Checkbox
                    id={`${dim.name}-${value}`}
                    checked={isSelected}
                    onCheckedChange={() => toggleValue(dimIndex, value)}
                  />
                  <Label
                    htmlFor={`${dim.name}-${value}`}
                    className="text-sm cursor-pointer"
                  >
                    {value}
                  </Label>
                </div>
              );
            })}
          </CardContent>
        </Card>
      ))}

      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="text-sm">Display Options</CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex items-center gap-2">
            <Checkbox id="show-orphans" defaultChecked />
            <Label htmlFor="show-orphans" className="text-sm">
              Show orphaned nodes
            </Label>
          </div>
          <div className="space-y-2">
            <Label className="text-sm">Node Size</Label>
            <Input
              type="range"
              defaultValue="100"
              min="50"
              max="200"
              className="w-full"
            />
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
