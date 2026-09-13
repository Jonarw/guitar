// Automated Guitar — expression marker generator for MuseScore 4.7.
//
// Scans the score for special staff texts on the six string staves and
// (re)writes a hidden marker staff whose notes encode, per string, the
// currently latched playing expression. MuseScore's MIDI output then emits
// plain NoteOn messages on the marker staff's channel; midi_conductor
// latches the expression per string.
//
// Run manually before playback: Plugins → Apply Guitar Expressions.
// Re-run after editing staff texts.

import QtQuick
import MuseScore 3.0
import FileIO 3.0

MuseScore {
    title: "Apply Guitar Expressions"
    version: "0.1"
    description: "Generates expression marker notes on the hidden '" +
        markerPartName + "' staff from staff texts on the string staves."
    requiresScore: true

    // ------------------------------------------------------------------
    // Configuration (wire-format contract with midi_conductor)
    // ------------------------------------------------------------------

    // Staff-text content (trimmed, lowercase) → expression code.
    // Code order matches the Expression enum in midi_conductor/src/engine.rs:
    // Soft=0, Hard=1, FingerSlam=2, HardDampen=3.
    // Each expression also has a 1-letter alias: s/h/f/d.
    readonly property var expressionTexts: ({
        "soft": 0, "s": 0,
        "hard": 1, "h": 1,
        "slam": 2, "f": 2,
        "damp": 3, "d": 3
    })

    // Base MIDI pitch per string staff. staffIdx 0 = high e .. 5 = low E,
    // matching channel_to_string in midi_conductor (get_base_pitch order
    // reversed: staves are ordered top-down in the score).
    readonly property var basePitch: [64, 59, 55, 50, 45, 40]

    // Name of the part holding the marker staff. The marker staff must be
    // the LAST staff of the score so it lands on MIDI channel 7.
    readonly property string markerPartName: "expression"

    // Marker note duration (NoteOff is ignored by the receiver).
    readonly property int markerDurZ: 1
    readonly property int markerDurN: 16

    // ------------------------------------------------------------------

    // Log destination. MuseScore 4 does not forward console.log to stdout,
    // and openLog()/logn() are deprecated no-ops -> write the log ourselves
    // with the FileIO plugin API.
    property string logFile: "/tmp/expression_midi.log"
    property var logLines: []

    FileIO {
        id: logOut
    }

    function say(msg) {
        console.log(msg);
        logLines.push(msg);
    }

    onRun: {
        logLines = ["--- run at " + new Date().toISOString() + " ---"];
        try {
            applyExpressions();
        } catch (e) {
            say("ERROR: " + e + "\n" + (e.stack || ""));
        }
        logOut.source = logFile;
        logOut.write(logLines.join("\n") + "\n");
    }

    function applyExpressions() {
        if (!curScore) {
            say("No score open.");
            return;
        }

        var markers = scanMarkers();
        say("Found " + markers.length + " expression staff text(s).");

        var staffIdx = findMarkerStaff();
        if (staffIdx < 0) {
            say("No '" + markerPartName + "' part found. Add a staff " +
                "(Add → Instruments), rename its part to '" + markerPartName +
                "', and keep it LAST in the score so it lands on MIDI channel 7.");
            return;
        }
        say("Marker staff is staff index " + staffIdx + ".");

        curScore.startCmd("Apply guitar expressions");
        clearMarkerStaff(staffIdx);
        writeMarkers(staffIdx, markers);
        curScore.endCmd();

        say("Wrote " + markers.length + " marker note(s).");
    }

    // Scan all segments for matching staff texts.
    // Returns [{tick, staffIdx, code, text}] sorted by tick; duplicate
    // (tick, staffIdx) pairs: last one wins.
    function scanMarkers() {
        var byKey = {};
        var seg = curScore.firstSegment();
        while (seg) {
            var tick = seg.fraction ? seg.fraction.ticks : seg.tick;

            var anns = seg.annotations;
            if (anns) {
                for (var i = 0; i < anns.length; i++) {
                    var a = anns[i];
                    if (a.type !== Element.STAFF_TEXT)
                        continue;
                    var text = ("" + a.text).trim().toLowerCase();
                    if (!(text in expressionTexts))
                        continue;

                    var sIdx = a.staffIdx;
                    if (sIdx === undefined)
                        sIdx = Math.floor(a.track / 4);

                    var m = { tick: tick, staffIdx: sIdx,
                              code: expressionTexts[text], text: text };
                    byKey[tick + "|" + sIdx] = m;
                    say("  tick " + tick + ", staff " + sIdx +
                        ": '" + text + "' -> code " + m.code);
                }
            }
            seg = seg.next;
        }

        var markers = [];
        for (var key in byKey)
            markers.push(byKey[key]);
        markers.sort(function (x, y) { return x.tick - y.tick; });
        return markers;
    }

    // Staff index of the marker staff, assuming one staff per part and
    // parts ordered like the staves. Matches on the mixer name (partName),
    // the instrument's long/short name (as set in Staff/Part properties),
    // or the instrumentId (e.g. "acoustic-guitar" would not match, but a
    // user-renamed part will). -1 if not found.
    function findMarkerStaff() {
        for (var i = 0; i < curScore.parts.length; i++) {
            var p = curScore.parts[i];
            var names = [
                p.partName,
                p.longName,
                p.shortName,
                p.instrumentId,
                p.instruments && p.instruments.length ? p.instruments[0].longName : "",
                p.instruments && p.instruments.length ? p.instruments[0].shortName : ""
            ];
            say("part " + i + ": " + names.map(function (n) {
                return "'" + (n === undefined || n === null ? "" : n) + "'";
            }).join(", "));
            for (var j = 0; j < names.length; j++) {
                if (("" + (names[j] || "")).trim().toLowerCase() === markerPartName)
                    return i;
            }
        }
        return -1;
    }

    // Remove all existing notes on the marker staff (voice 1), restoring
    // rests, so re-runs are idempotent.
    function clearMarkerStaff(staffIdx) {
        var c = curScore.newCursor();
        c.staffIdx = staffIdx;
        c.voice = 0;
        c.rewind(Cursor.SCORE_START);

        // Collect first: don't mutate while iterating.
        var chords = [];
        do {
            var el = c.element;
            if (el && el.type === Element.CHORD)
                chords.push(el);
        } while (c.next());

        for (var i = 0; i < chords.length; i++)
            removeElement(chords[i]);

        say("Cleared " + chords.length + " old marker note(s).");
    }

    // Write marker notes; markers sharing a tick (expression change on
    // several strings at once) must form a CHORD: addNote(pitch) alone
    // would replace the previous note, addNote(pitch, true) extends it.
    function writeMarkers(staffIdx, markers) {
        var prevTick = -1;
        for (var i = 0; i < markers.length; i++) {
            var m = markers[i];
            if (m.staffIdx < 0 || m.staffIdx >= basePitch.length) {
                say("  skipping text at tick " + m.tick +
                    ": staff " + m.staffIdx + " is not a string staff.");
                continue;
            }
            var pitch = basePitch[m.staffIdx] + m.code;
            insertMarkerNote(staffIdx, m.tick, pitch, m.text,
                             m.tick === prevTick);
            prevTick = m.tick;
        }
    }

    function insertMarkerNote(staffIdx, tick, pitch, text, addToChord) {
        var c = curScore.newCursor();
        c.staffIdx = staffIdx;
        c.voice = 0;
        c.setDuration(markerDurZ, markerDurN);
        // NOTE: rewindToTick(t) searches for t+1 with tick2leftSegment
        // ("integer ticks may contain numeric errors") and then advances to
        // the next element in the track -- on an empty marker staff that
        // lands on the next barline. rewindToFraction does an exact lookup.
        c.rewindToFraction(fractionFromTicks(tick));
        var seg = c.segment;
        if (seg) {
            var segTick = seg.fraction ? seg.fraction.ticks : -1;
            if (segTick !== tick)
                say("  WARNING: tick " + tick + " snapped to segment at " +
                    segTick + " (staff text sits between beats?)");
        } else {
            say("  WARNING: no segment found at tick " + tick +
                " -- marker will be dropped or misplaced");
        }
        c.addNote(pitch, addToChord);
        say("  marker at tick " + tick + ": pitch " + pitch +
            (addToChord ? " (chord)" : "") +
            " (string staff " + staffIdx + ", " + text + ")");
    }
}
