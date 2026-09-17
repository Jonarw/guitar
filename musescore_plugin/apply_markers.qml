import QtQuick
import MuseScore 3.0
import FileIO 3.0

MuseScore {
    title: "Automated Guitar Marker Generator"
    version: "0.1"
    description: "Generates hidden staves with marker notes for slurs and expressions that are consumed by the midi_conductor playback engine."
    requiresScore: true

    readonly property var expressionTexts: ({
        "soft": 0, "s": 0,
        "hard": 1, "h": 1,
        "slam": 2, "f": 2,
        "damp": 3, "d": 3
    });

    function expressionTextValue(chord) {
        var annotations = chord.parent.annotations;

        for (var i = 0; i < annotations.length; i++) {
            var element = annotations[i];
            if (element.staffIdx != chord.staffIdx)
                continue;

            if (element.type !== Element.STAFF_TEXT)
                continue;

            var text = element.text.trim().toLowerCase();

            if (expressionTexts.hasOwnProperty(text))
                return expressionTexts[text];
        }

        return null;
    }

    function clearAutoParts() {
        var autoParts = [];
        for (var i = 0; i < curScore.parts.length; i++) {
            var part = curScore.parts[i];
            if (part.shortName.startsWith("_meta")) {
                autoParts.push(part);
            }
        }

        curScore.removeParts(autoParts);
    }

    function makeAutoParts() {
        var length = curScore.parts.length;
        for (var i = 0; i < length; i++) {
            var part = curScore.parts[i];
            curScore.appendPart("guitar-nylon");
            var newPart = curScore.parts[curScore.parts.length - 1];
            curScore.setInstrumentName(newPart, fraction(0, 1), "_meta" + part.longName);
            curScore.setInstrumentAbbreviature(newPart, fraction(0, 1), "_meta" + part.shortName);
            newPart.staves[0].visible = false;
        }
    }

    function slurKey(element) {
        return element.staffIdx + ":" + element.parent.tick;
    }

    function buildSlurIndex() {
        var starts = {};
        var ends = {};

        var spanners = curScore.spanners;

        for (var i = 0; i < spanners.length; ++i) {
            var s = spanners[i];

            if (s.type !== Element.SLUR)
                continue;

            if (s.startElement) {
                starts[slurKey(s.startElement)] = true;
            }

            if (s.endElement) {
                ends[slurKey(s.endElement)] = true;
            }
        }

        return { starts: starts, ends: ends };
    }

    function processStaff(sourceStaffIdx, destinationStaffIdx, slurIndex) {
        var srcCursor = curScore.newCursor();
        srcCursor.staffIdx = sourceStaffIdx;
        srcCursor.rewind(Cursor.SCORE_START);

        var dstCursor = curScore.newCursor();
        dstCursor.staffIdx = destinationStaffIdx;
        dstCursor.rewind(Cursor.SCORE_START);

        var activeTupletTick = -1;

        var spanners = curScore.spanners;

        var BASE_NOTE = 52;
        var REST = BASE_NOTE;
        var NOTE = BASE_NOTE + 1;
        var SLUR_START = NOTE + 1;
        var SLUR_END = SLUR_START + 1;
        var EXPRESSION_BASE = SLUR_END + 1;

        while (srcCursor.element && dstCursor.element) {
            var src = srcCursor.element;

            if (src.type === Element.CHORD || src.type === Element.REST) {

                var srcTuplet = src.tuplet;

                if (srcTuplet && srcTuplet.fraction.ticks != activeTupletTick) {
                    dstCursor.addTuplet(
                        fraction(srcTuplet.actualNotes,srcTuplet.normalNotes),
                        srcTuplet.duration
                    );

                    activeTupletTick = srcTuplet.fraction.ticks;
                }

                dstCursor.setDuration(src.duration.numerator, src.duration.denominator);

                if (src.type === Element.REST) {
                    dstCursor.addNote(REST, false);
                } else {
                    dstCursor.addNote(NOTE, false);
                }

                var key = slurKey(src);

                if (slurIndex.starts[key]) {
                    dstCursor.prev();
                    dstCursor.addNote(SLUR_START, true);
                }

                if (slurIndex.ends[key]) {
                    dstCursor.prev();
                    dstCursor.addNote(SLUR_END, true);
                }

                var expression = expressionTextValue(src);
                if (expression !== null) {
                    dstCursor.prev();
                    dstCursor.addNote(EXPRESSION_BASE + expression, true);
                }
            }

            if (!srcCursor.next())
                break;
        }
    }

    function runStuff() {
        clearAutoParts();
        makeAutoParts();

        var count = curScore.parts.length / 2;
        var slurIndex = buildSlurIndex();
        for (var i = 0; i < count; i++) {
            processStaff(i, i + count, slurIndex);
        }
    }

    onRun: {
        curScore.startCmd();
        try {
            runStuff();
        } finally {
            curScore.endCmd();            
        }
    }
}
