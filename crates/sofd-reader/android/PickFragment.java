// The system picker answers on the activity that asked for it, and a
// NativeActivity has no onActivityResult for the answer to arrive in. A
// fragment does, and it attaches to the activity that already exists rather
// than replacing it, so the manifest keeps naming the activity it names.
package org.sofd.reader;

import android.app.Activity;
import android.app.Fragment;
import android.app.FragmentManager;
import android.content.Intent;
import android.net.Uri;
import android.os.Bundle;

public class PickFragment extends Fragment {
    private static final String TAG = "sofd-pick";
    private static final int REQUEST = 0x0f0d;
    private static final String MIME = "application/ofd";

    /** Asked from the reader's own thread, which may not touch a fragment. */
    public static void request(final Activity activity) {
        activity.runOnUiThread(new Runnable() {
            @Override
            public void run() {
                FragmentManager manager = activity.getFragmentManager();
                Fragment attached = manager.findFragmentByTag(TAG);
                if (attached != null) {
                    ((PickFragment) attached).pick();
                    return;
                }
                manager.beginTransaction()
                    .add(new PickFragment(), TAG)
                    .commitAllowingStateLoss();
            }
        });
    }

    @Override
    public void onCreate(Bundle state) {
        super.onCreate(state);
        // Restored after a rotation: the picker is already answered or gone.
        if (state == null) {
            pick();
        }
    }

    @Override
    public void onActivityResult(int requestCode, int resultCode, Intent data) {
        if (requestCode != REQUEST) {
            super.onActivityResult(requestCode, resultCode, data);
            return;
        }
        Uri uri = resultCode == Activity.RESULT_OK && data != null ? data.getData() : null;
        deliver(uri == null ? null : uri.toString());
    }

    private void pick() {
        Intent intent = new Intent(Intent.ACTION_GET_CONTENT);
        intent.addCategory(Intent.CATEGORY_OPENABLE);
        // Every type and then the two worth showing: a `.ofd` is reported as
        // `application/ofd` only on the systems that know it, and as
        // `application/octet-stream` on the rest, and a picker narrowed to the
        // first alone offers nothing to choose on those.
        intent.setType("*/*");
        intent.putExtra(Intent.EXTRA_MIME_TYPES, new String[] { MIME, "application/octet-stream" });
        startActivityForResult(intent, REQUEST);
    }

    private static native void deliver(String uri);
}
