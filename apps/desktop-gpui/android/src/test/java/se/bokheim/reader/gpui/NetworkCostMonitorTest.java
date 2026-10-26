package se.bokheim.reader.gpui;

import static org.junit.Assert.assertEquals;
import static org.robolectric.Shadows.shadowOf;
import org.robolectric.shadows.ShadowNetwork;
import android.net.Network;
import android.net.NetworkCapabilities;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;
import org.junit.Test;
import org.junit.runner.RunWith;
import org.robolectric.RobolectricTestRunner;
import org.robolectric.annotation.Config;

@RunWith(RobolectricTestRunner.class)
@Config(sdk = 28)
public class NetworkCostMonitorTest {
    @Test public void capabilitiesFollowTheDefaultNetworkAndIgnoreStaleCallbacks() {
        List<Integer> observed = new ArrayList<>();
        NetworkCostMonitor monitor = new NetworkCostMonitor(observed::add);
        Network wifi = ShadowNetwork.newInstance(1);
        Network mobile = ShadowNetwork.newInstance(2);
        NetworkCapabilities unmetered = new NetworkCapabilities();
        shadowOf(unmetered).addCapability(NetworkCapabilities.NET_CAPABILITY_NOT_METERED);
        NetworkCapabilities metered = new NetworkCapabilities();
        shadowOf(metered).removeCapability(NetworkCapabilities.NET_CAPABILITY_NOT_METERED);
        monitor.onAvailable(wifi);
        monitor.onCapabilitiesChanged(wifi, unmetered);
        monitor.onAvailable(mobile);
        monitor.onCapabilitiesChanged(mobile, metered);
        monitor.onLost(wifi);
        monitor.onCapabilitiesChanged(wifi, unmetered);
        assertEquals(Arrays.asList(1, 2), observed);
        // Cost can change without the network identity changing.
        monitor.onCapabilitiesChanged(mobile, unmetered);
        monitor.onLost(mobile);
        assertEquals(Arrays.asList(1, 2, 1, 0), observed);
    }

    @Test public void wifiUsesReportedMeteringRatherThanTransportType() {
        List<Integer> observed = new ArrayList<>();
        NetworkCostMonitor monitor = new NetworkCostMonitor(observed::add);
        Network wifi = ShadowNetwork.newInstance(1);
        NetworkCapabilities capabilities = new NetworkCapabilities();
        shadowOf(capabilities).addTransportType(NetworkCapabilities.TRANSPORT_WIFI);
        shadowOf(capabilities).removeCapability(NetworkCapabilities.NET_CAPABILITY_NOT_METERED);
        monitor.onAvailable(wifi);
        monitor.onCapabilitiesChanged(wifi, capabilities);
        shadowOf(capabilities).addCapability(NetworkCapabilities.NET_CAPABILITY_NOT_METERED);
        monitor.onCapabilitiesChanged(wifi, capabilities);
        assertEquals(Arrays.asList(2, 1), observed);
    }

    @Test @Config(sdk = 30)
    public void temporaryUnmeteredCapabilityAllowsTransfersUntilRevoked() {
        List<Integer> observed = new ArrayList<>();
        NetworkCostMonitor monitor = new NetworkCostMonitor(observed::add);
        Network mobile = ShadowNetwork.newInstance(2);
        NetworkCapabilities capabilities = new NetworkCapabilities();
        shadowOf(capabilities).addTransportType(NetworkCapabilities.TRANSPORT_CELLULAR);
        shadowOf(capabilities).removeCapability(NetworkCapabilities.NET_CAPABILITY_NOT_METERED);
        monitor.onAvailable(mobile);
        monitor.onCapabilitiesChanged(mobile, capabilities);
        shadowOf(capabilities).addCapability(NetworkCapabilities.NET_CAPABILITY_TEMPORARILY_NOT_METERED);
        monitor.onCapabilitiesChanged(mobile, capabilities);
        shadowOf(capabilities).removeCapability(NetworkCapabilities.NET_CAPABILITY_TEMPORARILY_NOT_METERED);
        monitor.onCapabilitiesChanged(mobile, capabilities);
        assertEquals(Arrays.asList(2, 1, 2), observed);
    }
}
