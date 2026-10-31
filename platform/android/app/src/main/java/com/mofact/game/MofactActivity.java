package com.mofact.game;

import org.libsdl.app.SDLActivity;

/** SDL's activity, loading the game: SDL is linked statically into libmain.so. */
public class MofactActivity extends SDLActivity {
    @Override
    protected String[] getLibraries() {
        return new String[] { "main" };
    }
}
