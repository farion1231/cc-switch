import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { toast } from "sonner";
import { Server, Play, Square, RefreshCw, Wifi } from "lucide-react";

interface ApiKey {
  id: string;
  provider: string;
  name: string;
  isActive: boolean;
  quotaUsed?: number;
  quotaLimit?: number;
}

interface MuxerStatus {
  isRunning: boolean;
  port: number;
  activeConnections: number;
  uptime: number;
}

interface CarrierMetric {
  interface: string;
  latency: number;
  quality: number;
}

export function MuxerPanel() {
  const [keys, setKeys] = useState<ApiKey[]>([]);
  const [status, setStatus] = useState<MuxerStatus | null>(null);
  const [carriers, setCarriers] = useState<CarrierMetric[]>([]);
  const [selectedProvider, setSelectedProvider] = useState<string>("openai");
  const [muxerPort, setMuxerPort] = useState(8888);
  const [isAddingKey, setIsAddingKey] = useState(false);
  const [newKeyName, setNewKeyName] = useState("");
  const [newKeyValue, setNewKeyValue] = useState("");

  useEffect(() => {
    loadKeys();
    loadStatus();
    loadCarriers();
  }, []);

  const loadKeys = async () => {
    try {
      const result = await invoke("list_api_keys");
      setKeys(result as ApiKey[]);
    } catch (error) {
      console.error("Failed to load API keys:", error);
      toast.error("Failed to load API keys");
    }
  };

  const loadStatus = async () => {
    try {
      const result = await invoke("get_muxer_status");
      setStatus(result as MuxerStatus);
    } catch (error) {
      console.error("Failed to load muxer status:", error);
    }
  };

  const loadCarriers = async () => {
    try {
      const result = await invoke("get_carrier_metrics");
      setCarriers(result as CarrierMetric[]);
    } catch (error) {
      console.error("Failed to load carrier metrics:", error);
    }
  };

  const handleAddKey = async () => {
    if (!newKeyName.trim() || !newKeyValue.trim()) {
      toast.error("Please enter key name and value");
      return;
    }

    setIsAddingKey(true);

    try {
      await invoke("add_api_key", {
        provider: selectedProvider,
        name: newKeyName,
        key: newKeyValue,
      });
      toast.success("API key added successfully");
      setNewKeyName("");
      setNewKeyValue("");
      await loadKeys();
    } catch (error) {
      console.error("Failed to add API key:", error);
      toast.error("Failed to add API key");
    } finally {
      setIsAddingKey(false);
    }
  };

  const handleRemoveKey = async (keyId: string) => {
    try {
      await invoke("remove_api_key", { keyId });
      toast.success("API key removed");
      await loadKeys();
    } catch (error) {
      console.error("Failed to remove API key:", error);
      toast.error("Failed to remove API key");
    }
  };

  const handleStartMuxer = async () => {
    try {
      await invoke("start_muxer", { port: muxerPort });
      toast.success("Muxer started");
      await loadStatus();
    } catch (error) {
      console.error("Failed to start muxer:", error);
      toast.error("Failed to start muxer");
    }
  };

  const handleStopMuxer = async () => {
    try {
      await invoke("stop_muxer");
      toast.success("Muxer stopped");
      await loadStatus();
    } catch (error) {
      console.error("Failed to stop muxer:", error);
      toast.error("Failed to stop muxer");
    }
  };

  const handleRefreshStatus = async () => {
    await loadStatus();
    await loadCarriers();
    toast.success("Status refreshed");
  };

  return (
    <div className="space-y-6">
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Server className="h-5 w-5" />
            Model Muxer
          </CardTitle>
          <CardDescription>
            Unified LLM API proxy with intelligent routing and quota management
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-2">
              <Badge variant={status?.isRunning ? "default" : "secondary"}>
                {status?.isRunning ? "Running" : "Stopped"}
              </Badge>
              {status && (
                <span className="text-sm text-muted-foreground">
                  Port {status.port} • {status.activeConnections} connections •
                  Uptime {Math.floor(status.uptime / 1000)}s
                </span>
              )}
            </div>
            <div className="flex items-center gap-2">
              <Button
                onClick={status?.isRunning ? handleStopMuxer : handleStartMuxer}
                variant={status?.isRunning ? "destructive" : "default"}
                className="gap-2"
              >
                {status?.isRunning ? (
                  <>
                    <Square className="h-4 w-4" />
                    Stop
                  </>
                ) : (
                  <>
                    <Play className="h-4 w-4" />
                    Start
                  </>
                )}
              </Button>
              <Button
                variant="outline"
                size="icon"
                onClick={handleRefreshStatus}
              >
                <RefreshCw className="h-4 w-4" />
              </Button>
            </div>
          </div>

          <Tabs defaultValue="keys" className="w-full">
            <TabsList>
              <TabsTrigger value="keys">API Keys</TabsTrigger>
              <TabsTrigger value="carriers">Carriers</TabsTrigger>
              <TabsTrigger value="config">Config</TabsTrigger>
            </TabsList>

            <TabsContent value="keys" className="space-y-4">
              <div className="flex gap-4">
                <div className="flex-1">
                  <Label>Provider</Label>
                  <Select
                    value={selectedProvider}
                    onValueChange={setSelectedProvider}
                  >
                    <SelectTrigger>
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="openai">OpenAI</SelectItem>
                      <SelectItem value="anthropic">Anthropic</SelectItem>
                      <SelectItem value="google">Google</SelectItem>
                      <SelectItem value="deepseek">DeepSeek</SelectItem>
                    </SelectContent>
                  </Select>
                </div>
                <div className="flex-1">
                  <Label>Muxer Port</Label>
                  <Input
                    type="number"
                    value={muxerPort}
                    onChange={(e) =>
                      setMuxerPort(parseInt(e.target.value) || 8888)
                    }
                    placeholder="8888"
                  />
                </div>
              </div>

              <div className="space-y-2">
                <Label>API Keys</Label>
                <div className="text-sm text-muted-foreground mb-2">
                  Manage your API keys for different providers. Keys are
                  encrypted and stored locally.
                </div>
                <div className="flex gap-2">
                  <Input
                    placeholder="Key name"
                    value={newKeyName}
                    onChange={(e) => setNewKeyName(e.target.value)}
                    className="flex-1"
                  />
                  <Input
                    type="password"
                    placeholder="API key value"
                    value={newKeyValue}
                    onChange={(e) => setNewKeyValue(e.target.value)}
                    className="flex-1"
                  />
                  <Button onClick={handleAddKey} disabled={isAddingKey}>
                    {isAddingKey ? "Adding..." : "Add Key"}
                  </Button>
                </div>
              </div>

              <div className="border rounded-md">
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>Provider</TableHead>
                      <TableHead>Name</TableHead>
                      <TableHead>Status</TableHead>
                      <TableHead>Quota</TableHead>
                      <TableHead>Actions</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {keys.length === 0 ? (
                      <TableRow>
                        <TableCell
                          colSpan={5}
                          className="text-center text-muted-foreground"
                        >
                          No API keys configured
                        </TableCell>
                      </TableRow>
                    ) : (
                      keys.map((key) => (
                        <TableRow key={key.id}>
                          <TableCell>
                            <Badge variant="outline">{key.provider}</Badge>
                          </TableCell>
                          <TableCell>{key.name}</TableCell>
                          <TableCell>
                            <Badge
                              variant={key.isActive ? "default" : "secondary"}
                            >
                              {key.isActive ? "Active" : "Inactive"}
                            </Badge>
                          </TableCell>
                          <TableCell>
                            {key.quotaLimit && (
                              <span className="text-sm">
                                {Math.round(
                                  ((key.quotaUsed || 0) / key.quotaLimit) * 100,
                                )}
                                %
                              </span>
                            )}
                          </TableCell>
                          <TableCell>
                            <Button
                              variant="ghost"
                              size="sm"
                              onClick={() => handleRemoveKey(key.id)}
                            >
                              Remove
                            </Button>
                          </TableCell>
                        </TableRow>
                      ))
                    )}
                  </TableBody>
                </Table>
              </div>
            </TabsContent>

            <TabsContent value="carriers" className="space-y-4">
              <div className="grid grid-cols-2 gap-4">
                {carriers.map((carrier) => (
                  <Card key={carrier.interface}>
                    <CardHeader>
                      <CardTitle className="flex items-center gap-2">
                        <Wifi className="h-4 w-4" />
                        {carrier.interface}
                      </CardTitle>
                    </CardHeader>
                    <CardContent>
                      <div className="space-y-2">
                        <div className="flex justify-between text-sm">
                          <span>Latency</span>
                          <span>{carrier.latency?.toFixed(0) || "-"} ms</span>
                        </div>
                        <div className="flex justify-between text-sm">
                          <span>Quality</span>
                          <span>{(carrier.quality * 100).toFixed(0)}%</span>
                        </div>
                      </div>
                    </CardContent>
                  </Card>
                ))}
              </div>
            </TabsContent>

            <TabsContent value="config" className="space-y-4">
              <div className="text-sm text-muted-foreground">
                Muxer configuration will be available in a future update.
              </div>
            </TabsContent>
          </Tabs>
        </CardContent>
      </Card>
    </div>
  );
}
