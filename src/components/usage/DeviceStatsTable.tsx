import { useTranslation } from "react-i18next";
import { MonitorSmartphone } from "lucide-react";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { useQuery } from "@tanstack/react-query";
import { usageApi } from "@/lib/api/usage";
import { usageKeys } from "@/lib/query/usage";
import { resolveUsageRange } from "@/lib/usageRange";
import { fmtUsd } from "./format";
import type { UsageRangeSelection } from "@/types/usage";

interface DeviceStatsTableProps {
  range: UsageRangeSelection;
  appType?: string;
  providerName?: string;
  model?: string;
  refreshIntervalMs: number;
}

/**
 * 设备维度用量统计表（v12+）。
 *
 * 当未筛选设备时展示各设备的用量对比；筛选了具体设备后此表仍可用（只显示该设备一行）。
 * 数据来自 `get_usage_summary_by_device`，按真实 token 总量降序。
 */
export function DeviceStatsTable({
  range,
  appType,
  providerName,
  model,
  refreshIntervalMs,
}: DeviceStatsTableProps) {
  const { t } = useTranslation();
  const { startDate, endDate } = resolveUsageRange(range);

  const { data: stats, isLoading } = useQuery({
    queryKey: [
      ...usageKeys.all,
      "summaryByDevice",
      range.preset,
      range.customStartDate,
      range.customEndDate,
      appType,
      providerName,
      model,
      range.liveEndTime,
    ],
    queryFn: () =>
      usageApi.getUsageSummaryByDevice(
        startDate,
        endDate,
        appType && appType !== "all" ? appType : undefined,
        providerName,
        model,
      ),
    refetchInterval: refreshIntervalMs > 0 ? refreshIntervalMs : false,
    refetchIntervalInBackground: false,
  });

  // 设备名映射：尝试从远端注册表拿友好名，否则用 deviceId 截断显示
  const { data: deviceRegistry } = useQuery({
    queryKey: ["usage-sync-devices"],
    queryFn: () => usageApi.usageSyncFetchDevices(),
    retry: false,
    staleTime: 5 * 60 * 1000,
  });
  const deviceNameMap = new Map(
    (deviceRegistry ?? []).map((d) => [d.deviceId, d.deviceName]),
  );
  const resolveDeviceLabel = (id: string) => {
    if (!id) return t("usage.thisDevice", "This Device");
    return deviceNameMap.get(id) ?? id.slice(0, 8);
  };

  if (isLoading) {
    return <div className="h-[400px] animate-pulse rounded bg-gray-100" />;
  }

  return (
    <div className="rounded-lg border border-border/50 bg-card/40 backdrop-blur-sm overflow-hidden">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>
              <span className="inline-flex items-center gap-1.5">
                <MonitorSmartphone className="h-3.5 w-3.5" />
                {t("usage.device", "Device")}
              </span>
            </TableHead>
            <TableHead className="text-right">
              {t("usage.requests", "Requests")}
            </TableHead>
            <TableHead className="text-right">
              {t("usage.tokens", "Tokens")}
            </TableHead>
            <TableHead className="text-right">
              {t("usage.cost", "Cost")}
            </TableHead>
            <TableHead className="text-right">
              {t("usage.successRate", "Success Rate")}
            </TableHead>
            <TableHead className="text-right">
              {t("usage.cacheHitRate", "Cache Hit")}
            </TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {stats?.length === 0 ? (
            <TableRow>
              <TableCell
                colSpan={6}
                className="text-center text-muted-foreground"
              >
                {t("usage.noData", "No data")}
              </TableCell>
            </TableRow>
          ) : (
            stats?.map((stat) => (
              <TableRow key={stat.deviceId}>
                <TableCell className="font-medium">
                  {resolveDeviceLabel(stat.deviceId)}
                </TableCell>
                <TableCell className="text-right">
                  {stat.summary.totalRequests.toLocaleString()}
                </TableCell>
                <TableCell className="text-right">
                  {stat.summary.realTotalTokens.toLocaleString()}
                </TableCell>
                <TableCell className="text-right">
                  {fmtUsd(stat.summary.totalCost, 4)}
                </TableCell>
                <TableCell className="text-right">
                  {stat.summary.successRate.toFixed(1)}%
                </TableCell>
                <TableCell className="text-right">
                  {(stat.summary.cacheHitRate * 100).toFixed(1)}%
                </TableCell>
              </TableRow>
            ))
          )}
        </TableBody>
      </Table>
      <p className="px-4 py-2 text-xs text-muted-foreground">
        {t(
          "usage.deviceStatsHint",
          "Per-device usage requires enabling usage sync (shares S3 sync credentials). Each device only uploads its own data.",
        )}
      </p>
    </div>
  );
}
