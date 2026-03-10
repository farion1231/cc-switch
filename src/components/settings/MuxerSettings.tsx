import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";

export function MuxerSettings() {
  const [deploying, setDeploying] = useState(false);
  const [litebikeUrl, setLitebikeUrl] = useState("http://localhost:8889");
  const [muxerPort, setMuxerPort] = useState(8888);
  const [keys, setKeys] = useState<{ provider: string; name: string }[]>([]);
  const [status, setStatus] = useState<
    "idle" | "deploying" | "running" | "error"
  >("idle");

  const handleDeploy = async () => {
    setDeploying(true);
    setStatus("deploying");

    try {
      await new Promise((resolve) => setTimeout(resolve, 2000));
      setStatus("running");
    } catch {
      setStatus("error");
    } finally {
      setDeploying(false);
    }
  };

  const handleStop = async () => {
    setStatus("idle");
  };

  const handleAddKey = () => {
    setKeys([...keys, { provider: "openai", name: `Key ${keys.length + 1}` }]);
  };

  const handleDeleteKey = (index: number) => {
    setKeys(keys.filter((_, i) => i !== index));
  };

  return (
    <div className="space-y-6">
      <Card>
        <CardHeader>
          <div className="flex items-center justify-between">
            <CardTitle>LiteBike Daemon</CardTitle>
            <Badge variant={status === "running" ? "default" : "secondary"}>
              {status}
            </Badge>
          </div>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="grid grid-cols-2 gap-4">
            <div className="space-y-2">
              <Label htmlFor="muxer-port">Muxer Port</Label>
              <Input
                id="muxer-port"
                type="number"
                value={muxerPort}
                onChange={(e) => setMuxerPort(parseInt(e.target.value) || 8888)}
                disabled={status === "running"}
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="litebike-url">LiteBike URL</Label>
              <Input
                id="litebike-url"
                type="text"
                value={litebikeUrl}
                onChange={(e) => setLitebikeUrl(e.target.value)}
              />
            </div>
          </div>

          <div className="space-y-2">
            <Label>API Keys ({keys.length})</Label>
            <div className="space-y-2 max-h-40 overflow-y-auto border rounded-md p-2">
              {keys.length === 0 ? (
                <p className="text-sm text-muted-foreground">
                  No keys configured
                </p>
              ) : (
                keys.map((key, i) => (
                  <div
                    key={i}
                    className="flex items-center justify-between p-2"
                  >
                    <div className="flex items-center gap-2">
                      <Badge variant="outline">{key.provider}</Badge>
                      <span className="text-sm">{key.name}</span>
                    </div>
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => handleDeleteKey(i)}
                    >
                      Delete
                    </Button>
                  </div>
                ))
              )}
            </div>
            <Button variant="outline" onClick={handleAddKey} className="w-full">
              Add Key
            </Button>
          </div>

          <div className="flex justify-end gap-2">
            <Button
              variant="outline"
              onClick={handleStop}
              disabled={status !== "running"}
            >
              Stop
            </Button>
            <Button
              onClick={handleDeploy}
              disabled={deploying || status === "running"}
            >
              {deploying ? "Deploying..." : "Deploy LiteBike"}
            </Button>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
