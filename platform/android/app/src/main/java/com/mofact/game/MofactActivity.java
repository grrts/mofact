package com.mofact.game;

import com.revenuecat.purchases.CustomerInfo;
import com.revenuecat.purchases.EntitlementInfo;
import com.revenuecat.purchases.Offering;
import com.revenuecat.purchases.Offerings;
import com.revenuecat.purchases.Package;
import com.revenuecat.purchases.PurchaseParams;
import com.revenuecat.purchases.Purchases;
import com.revenuecat.purchases.PurchasesConfiguration;
import com.revenuecat.purchases.PurchasesError;
import com.revenuecat.purchases.interfaces.PurchaseCallback;
import com.revenuecat.purchases.interfaces.ReceiveCustomerInfoCallback;
import com.revenuecat.purchases.interfaces.ReceiveOfferingsCallback;
import com.revenuecat.purchases.models.StoreTransaction;
import android.os.Build;
import android.view.HapticFeedbackConstants;
import org.libsdl.app.SDLActivity;

/**
 * SDL's activity, loading the game: SDL is linked statically into libmain.so.
 *
 * <p>It also sells the full game through RevenueCat (T61). The game calls the
 * {@code store*} methods from its own thread; answers come back through
 * {@link #nativeStoreEvent}, which the game reads once a frame. The RevenueCat
 * key is built in from the {@code mofactRevenueCatKey} Gradle property or the
 * {@code MOFACT_REVENUECAT_KEY} environment variable; without one the game
 * uses its free sandbox instead.
 */
public class MofactActivity extends SDLActivity {
    /** The entitlement that is the full game, as set up in RevenueCat. */
    static final String ENTITLEMENT = "full_game";

    /** Event kinds, matching mofact-android's store.rs. */
    static final int PRICE = 0, OWNED = 1, CANCELLED = 2, NOTHING_TO_RESTORE = 3, FAILED = 4;

    /** The full game's package, once the store has listed it. */
    private volatile Package fullGame;

    static native void nativeStoreEvent(int kind, String text);

    @Override
    protected String[] getLibraries() {
        return new String[] { "main" };
    }

    /**
     * Play one of the game's feels (mofact::haptics::Haptic): 0 a tick, 1 a
     * warning, 2 a success. The phone's touch-feedback setting decides.
     */
    public boolean haptic(int feel) {
        final int effect;
        boolean modern = Build.VERSION.SDK_INT >= Build.VERSION_CODES.R;
        if (feel == 1) {
            effect = modern ? HapticFeedbackConstants.REJECT : HapticFeedbackConstants.LONG_PRESS;
        } else if (feel == 2) {
            effect = modern ? HapticFeedbackConstants.CONFIRM : HapticFeedbackConstants.LONG_PRESS;
        } else {
            effect = HapticFeedbackConstants.CLOCK_TICK;
        }
        runOnUiThread(() -> getWindow().getDecorView().performHapticFeedback(effect));
        return true;
    }

    private static boolean owns(CustomerInfo info) {
        EntitlementInfo e = info.getEntitlements().get(ENTITLEMENT);
        return e != null && e.isActive();
    }

    private static String why(PurchasesError e) {
        return "The store said: " + e.getMessage();
    }

    /** Set RevenueCat up and ask what is owned and what it costs. False without a key. */
    public boolean storeStart() {
        final String key = BuildConfig.REVENUECAT_KEY;
        if (key.isEmpty()) {
            return false;
        }
        runOnUiThread(() -> {
            Purchases.configure(new PurchasesConfiguration.Builder(this, key).build());
            Purchases.getSharedInstance().getCustomerInfo(new ReceiveCustomerInfoCallback() {
                @Override
                public void onReceived(CustomerInfo info) {
                    if (owns(info)) {
                        nativeStoreEvent(OWNED, "");
                    }
                }

                @Override
                public void onError(PurchasesError error) {
                    // Offline: the game goes by what it remembers.
                }
            });
            Purchases.getSharedInstance().getOfferings(new ReceiveOfferingsCallback() {
                @Override
                public void onReceived(Offerings offerings) {
                    Offering current = offerings.getCurrent();
                    if (current == null || current.getAvailablePackages().isEmpty()) {
                        return;
                    }
                    Package p = current.getLifetime();
                    if (p == null) {
                        p = current.getAvailablePackages().get(0);
                    }
                    fullGame = p;
                    nativeStoreEvent(PRICE, p.getProduct().getPrice().getFormatted());
                }

                @Override
                public void onError(PurchasesError error) {}
            });
        });
        return true;
    }

    /** Start buying the full game. */
    public boolean storeBuy() {
        runOnUiThread(() -> {
            Package p = fullGame;
            if (p == null) {
                nativeStoreEvent(FAILED, "The store couldn't be reached. Try again later.");
                return;
            }
            PurchaseParams params = new PurchaseParams.Builder(this, p).build();
            Purchases.getSharedInstance().purchase(params, new PurchaseCallback() {
                @Override
                public void onCompleted(StoreTransaction transaction, CustomerInfo info) {
                    if (owns(info)) {
                        nativeStoreEvent(OWNED, "");
                    } else {
                        nativeStoreEvent(FAILED, "The purchase is still pending.");
                    }
                }

                @Override
                public void onError(PurchasesError error, boolean userCancelled) {
                    if (userCancelled) {
                        nativeStoreEvent(CANCELLED, "");
                    } else {
                        nativeStoreEvent(FAILED, why(error));
                    }
                }
            });
        });
        return true;
    }

    /** Look for the full game bought before on this account. */
    public boolean storeRestore() {
        runOnUiThread(() -> Purchases.getSharedInstance().restorePurchases(
                new ReceiveCustomerInfoCallback() {
                    @Override
                    public void onReceived(CustomerInfo info) {
                        nativeStoreEvent(owns(info) ? OWNED : NOTHING_TO_RESTORE, "");
                    }

                    @Override
                    public void onError(PurchasesError error) {
                        nativeStoreEvent(FAILED, why(error));
                    }
                }));
        return true;
    }
}
