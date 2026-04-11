import { useState, useEffect, useMemo } from 'react';
import { toast } from 'sonner';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { useRegisterMCPServer } from '@/hooks/useMCPServers';
import { Server, AlertTriangle } from 'lucide-react';
import { cn } from '@/lib/utils';
import yaml from 'js-yaml';

interface MCPServerDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function MCPServerDialog({ open, onOpenChange }: MCPServerDialogProps) {
  const registerServer = useRegisterMCPServer();
  const [manifestText, setManifestText] = useState('');
  const [parseError, setParseError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) {
      setManifestText('');
      setParseError(null);
    }
  }, [open]);

  const parsed = useMemo(() => {
    if (!manifestText.trim()) {
      setParseError(null);
      return null;
    }
    try {
      const obj = yaml.load(manifestText) as Record<string, unknown>;
      setParseError(null);
      return obj;
    } catch (err) {
      setParseError(err instanceof Error ? err.message : 'Invalid YAML');
      return null;
    }
  }, [manifestText]);

  const metadata = parsed?.metadata as Record<string, string> | undefined;
  const serverName = metadata?.name;

  async function handleRegister() {
    if (!parsed || !serverName) {
      toast.error('Please enter a valid MCP server manifest with metadata.name');
      return;
    }

    try {
      await registerServer.mutateAsync(parsed);
      toast.success(`MCP server "${serverName}" registered`);
      onOpenChange(false);
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Registration failed');
    }
  }

  const exampleManifest = `apiVersion: sera/v1
kind: SkillProvider
metadata:
  name: github-mcp
  description: GitHub API tool provider
image: ghcr.io/modelcontextprotocol/servers/github:latest
transport: stdio
network:
  allowlist:
    - api.github.com
secrets:
  - GITHUB_TOKEN`;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl max-h-[85vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Server size={16} className="text-sera-accent" />
            Register MCP Server
          </DialogTitle>
          <DialogDescription>
            MCP tool servers run as sandboxed Docker containers. Paste a server manifest below.
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4">
          {/* YAML editor */}
          <div>
            <label className="block text-xs font-medium text-sera-text-muted mb-1">
              Server Manifest (YAML)
            </label>
            <textarea
              value={manifestText}
              onChange={(e) => setManifestText(e.target.value)}
              placeholder={exampleManifest}
              className={cn(
                'w-full h-64 bg-sera-bg border rounded-lg p-3',
                'text-xs font-mono text-sera-text resize-y',
                'outline-none focus:border-sera-accent',
                'placeholder:text-sera-text-dim',
                parseError ? 'border-red-500' : 'border-sera-border'
              )}
            />
            {parseError && (
              <p className="text-[10px] text-red-400 mt-1 flex items-center gap-1">
                <AlertTriangle size={10} /> {parseError}
              </p>
            )}
          </div>

          {/* Parsed preview */}
          {parsed && (
            <div className="sera-card-static p-3 space-y-2">
              <h4 className="text-[10px] uppercase tracking-wider text-sera-text-dim font-bold">
                Parsed Manifest
              </h4>
              <div className="grid grid-cols-2 gap-2 text-xs">
                {serverName && (
                  <div>
                    <span className="text-sera-text-dim">Name:</span>{' '}
                    <span className="text-sera-text font-mono">{serverName}</span>
                  </div>
                )}
                {'image' in parsed && (
                  <div>
                    <span className="text-sera-text-dim">Image:</span>{' '}
                    <span className="text-sera-text font-mono text-[10px]">
                      {String(parsed['image'])}
                    </span>
                  </div>
                )}
                {'transport' in parsed && (
                  <div>
                    <span className="text-sera-text-dim">Transport:</span>{' '}
                    <Badge variant="accent">{String(parsed['transport'])}</Badge>
                  </div>
                )}
                {'kind' in parsed && (
                  <div>
                    <span className="text-sera-text-dim">Kind:</span>{' '}
                    <Badge variant="default">{String(parsed['kind'])}</Badge>
                  </div>
                )}
              </div>
            </div>
          )}

          {/* Actions */}
          <div className="flex justify-end gap-2 pt-2">
            <Button variant="ghost" size="sm" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <Button
              size="sm"
              onClick={() => {
                void handleRegister();
              }}
              disabled={!parsed || !serverName || registerServer.isPending}
            >
              {registerServer.isPending ? 'Registering…' : 'Register Server'}
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
