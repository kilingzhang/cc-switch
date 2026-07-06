import { useTranslation } from "react-i18next";
import { useMutation, useQuery } from "@tanstack/react-query";
import { Upload, Download, RefreshCw, CheckCircle2, AlertCircle } from "lucide-react";
import { Button } from "@/components/ui/button";
import { usageApi } from "@/lib/api/usage";
import { toast } from "sonner";
import type { S3SyncSettings, WebDavSyncSettings } from "@/types";

interface UsageSyncSectionProps {
  s3Config?: S3SyncSettings | null;
  webdavConfig?: WebDavSyncSettings | null;
}

/**
 * 跨设备用量同步设置（v12+）。
 *
 * 复用配置同步的 S3/WebDAV 凭证，但走独立的 usage/v1/ 远端路径：
 * - 上传：把本设备的用量汇总推到独立 slot
 * - 拉取：合并所有设备的用量到本地，仪表盘「设备」Tab 可见分设备数据
 *
 * 仅当 S3 或 WebDAV 同步已启用时才可用（当前仅实现 S3 传输）。
 */
export function UsageSyncSection({
  s3Config,
  webdavConfig,
}: UsageSyncSectionProps) {
  const { t } = useTranslation();
  const s3Enabled = !!s3Config?.enabled;
  const webdavEnabled = !!webdavConfig?.enabled;
  const anyEnabled = s3Enabled || webdavEnabled;

  const { data: devices, refetch } = useQuery({
    queryKey: ["usage-sync-devices"],
    queryFn: () => usageApi.usageSyncFetchDevices(),
    enabled: anyEnabled,
    retry: false,
    staleTime: 60 * 1000,
  });

  const uploadMutation = useMutation({
    mutationFn: () => usageApi.usageSyncUpload(),
    onSuccess: () => {
      toast.success(
        t("settings.usageSync.uploadSuccess", "Usage uploaded to remote"),
      );
      refetch();
    },
    onError: (e: unknown) => {
      toast.error(
        t("settings.usageSync.uploadFailed", "Upload failed") +
          ": " +
          String(e),
      );
    },
  });

  const downloadMutation = useMutation({
    mutationFn: () => usageApi.usageSyncDownloadAll(),
    onSuccess: (res) => {
      toast.success(
        t("settings.usageSync.downloadSuccess", {
          defaultValue: "Merged {{count}} devices",
          count: res.mergedDevices,
        }),
      );
    },
    onError: (e: unknown) => {
      toast.error(
        t("settings.usageSync.downloadFailed", "Download failed") +
          ": " +
          String(e),
      );
    },
  });

  if (!anyEnabled) {
    return (
      <div className="rounded-lg border border-dashed border-border/50 p-4 text-sm text-muted-foreground">
        {t(
          "settings.usageSync.disabledHint",
          "Enable S3 or WebDAV sync above to use cross-device usage sync.",
        )}
      </div>
    );
  }

  return (
    <div className="space-y-4">
      <div className="rounded-lg bg-muted/40 p-4 text-sm">
        <p className="text-muted-foreground">
          {t(
            "settings.usageSync.description",
            "Push this device's usage to a per-device remote slot and merge all devices' data locally. Shares the S3/WebDAV credentials configured above but uses an independent remote path (usage/v1/).",
          )}
        </p>
      </div>

      <div className="flex flex-wrap gap-2">
        <Button
          size="sm"
          variant="default"
          onClick={() => uploadMutation.mutate()}
          disabled={uploadMutation.isPending}
        >
          {uploadMutation.isPending ? (
            <RefreshCw className="h-3.5 w-3.5 animate-spin" />
          ) : (
            <Upload className="h-3.5 w-3.5" />
          )}
          {t("settings.usageSync.upload", "Upload this device")}
        </Button>
        <Button
          size="sm"
          variant="outline"
          onClick={() => downloadMutation.mutate()}
          disabled={downloadMutation.isPending}
        >
          {downloadMutation.isPending ? (
            <RefreshCw className="h-3.5 w-3.5 animate-spin" />
          ) : (
            <Download className="h-3.5 w-3.5" />
          )}
          {t("settings.usageSync.download", "Merge all devices")}
        </Button>
      </div>

      <div className="rounded-lg border border-border/50">
        <div className="px-4 py-2 border-b border-border/50 text-xs font-medium text-muted-foreground">
          {t("settings.usageSync.registeredDevices", "Registered devices")}
        </div>
        <div className="divide-y divide-border/30">
          {devices?.length === 0 ? (
            <div className="px-4 py-3 text-sm text-muted-foreground">
              {t("settings.usageSync.noDevices", "No devices registered yet")}
            </div>
          ) : (
            devices?.map((d) => (
              <div
                key={d.deviceId}
                className="flex items-center justify-between px-4 py-2 text-sm"
              >
                <span className="inline-flex items-center gap-2">
                  <CheckCircle2 className="h-3.5 w-3.5 text-green-500" />
                  {d.deviceName}
                </span>
                <span className="text-xs text-muted-foreground">
                  {new Date(d.lastUploadAt).toLocaleString()}
                </span>
              </div>
            ))
          )}
        </div>
      </div>

      {!s3Enabled && webdavEnabled && (
        <p className="inline-flex items-center gap-1.5 text-xs text-amber-600">
          <AlertCircle className="h-3.5 w-3.5" />
          {t(
            "settings.usageSync.webdavUnsupported",
            "WebDAV transport for usage sync is not yet implemented; only S3 is supported.",
          )}
        </p>
      )}
    </div>
  );
}
