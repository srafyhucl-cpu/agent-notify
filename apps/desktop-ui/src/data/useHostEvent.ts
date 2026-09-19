import { useQueryClient, type QueryClient } from "@tanstack/react-query";
import { useEffect } from "react";

import type { HostBridge } from "../bridge";
import { queryKeys } from "./queryKeys";

function invalidate(queryClient: QueryClient, queryKey: readonly unknown[]) {
  void queryClient.invalidateQueries({ queryKey });
}

export function useHostEvent(bridge: HostBridge) {
  const queryClient = useQueryClient();

  useEffect(() => {
    const unsubscribeSnapshot = bridge.subscribe("snapshot.changed", () => {
      invalidate(queryClient, queryKeys.snapshot());
      invalidate(queryClient, queryKeys.agents());
    });

    const unsubscribeDelivery = bridge.subscribe("delivery.changed", (event) => {
      invalidate(queryClient, queryKeys.snapshot());
      invalidate(queryClient, queryKeys.deliveries());
      invalidate(queryClient, queryKeys.notifications());
      if (event.notificationId) {
        invalidate(
          queryClient,
          queryKeys.notificationDetail(event.notificationId),
        );
      }
    });

    const unsubscribeLogin = bridge.subscribe(
      "channel.login.changed",
      (event) => {
        invalidate(queryClient, queryKeys.snapshot());
        invalidate(queryClient, queryKeys.channels());
        invalidate(queryClient, queryKeys.channelLogin(event.accountId));
      },
    );

    return () => {
      unsubscribeSnapshot();
      unsubscribeDelivery();
      unsubscribeLogin();
    };
  }, [bridge, queryClient]);
}
