\version "2.26.0"
\language "english"

\header
%comment
{
  title   =  
  %comment 
  "Angelica"
}


global = {
  \key c \major
 
}


stringOne = {
  \global
  \time 4/4 
  \tempo 4 = 112
 
  R1*8
  e'16\ff e' e'8 r4 r4 r8 a'16\f b'
  c''4 r8 f''16 ds'' e''4 r8 c''16 gs'
  a'4 r4 r2
  R1*4
  r2 \tuplet 6/4 { f''16\f e'' d'' c'' b' a' } r4
  R1
  a'4.\f gs' \tuplet 3/2 { g'16 a' g'~ } g'8
  f'4. e' r16 e' f'8
  e'-> r4 f'8-> r4 e'8-> r8
  e'-> r4 f'8-> r4 e'8-> r8
  a'4.\mf gs' \tuplet 3/2 { g'16 a' g'~ } g'8
  f'4. e' r16 e' f'8
  e'-> r4 f'8-> r4 e'8-> r8
  e'-> r4 f'8-> r4 e'8-> r8
  r8 e'16\f a' c'' e'' ds''8-> e'16 a' c'' f'' e''4->
  r8 e'16 a' c'' e'' ds''8-> e'16 a' c'' f'' a''4->
  a''4. gs'' g''16( f''8.)
  \tuplet 3/2 { f''16( g'' f''~ } f''4) e''4. c''4
  r8 e'16\mf a' c'' e'' ds''8-> e'16 a' c'' f'' e''4->
  r8 e'16 a' c'' e'' ds''8-> e'16 a' c'' f'' a''4->
  a''4. gs'' g''16( f''8.)
  \tuplet 3/2 { f''16( g'' f''~ } f''4) e''4. c''4
  r8 r16 f'\ff a' b'8 bf'16 a' r r8 f'8\mf \tuplet 3/2 { f'16 f' f' }
  f'8 r f' r f' r f' \tuplet 3/2 { e'16 e' e' }
  e'8 r e' r e' e'16\ff f' e'8 b'16 c''
  c''( b') b'( a') c''( a') a'( gs') gs' a'( gs') f' e'8 r
  e''8( d''4) e''8( d''4) r8 e''16 e''
  e'' f'' d'' e'' c'' e'' d'' e'' c'' d'' b' c'' a'8 \tuplet 3/2 { e'16\mf e' e' }
  e'8 r e' r e' r e' \tuplet 3/2 { e'16 e' e' }
  \tuplet 6/4 { gs'16\f a' b' a' b' b' } \tuplet 3/2 { d'' f'' e'' } d''32 d'' c'' b' \tuplet 6/4 { a'16 f'' e'' d'' c'' b' } a'32 b' a' gs' \tuplet 3/2 { f'16 f' e' }
  
  g'8\mp f'32( g' f'8.) g'8 f'32( g' f'4..)
  g'8 f'32( g' f'8.) g'8 f'32( g' f'8.) r4
  R1*9
  r2.. a'16\f b'
  c''1
  R1*4
  r4. \tuplet 3/2 { r16 f' a' } \tuplet 3/2 { b'4 a'32 b' a'16~ } a'8 r
  e'4 r8 e'8\p \tuplet 6/4 { f''16\f e'' d'' c'' b' a' } \tuplet 6/4 { b' a' g' f' r8 }
  r8 e'16\mp r8 f'16 r8 fs'16 r8 g'16 r8 gs'
  
  f''8 d'' a' f''~ f'' d'' a'4
  f''8 d'' a' f'' d'' a' e'' d''
  e'' c'' a' e''~ e'' c'' a'4
  e''8 c'' a' e'' c'' a' e'' r
  ds'' b' fs' ds'' b' fs' ds'' r
  d'' b' f' d'' b' f' d'' r
  c'' a' e' c'' a' e' c'' r
}

stringTwo = {
\global
  R1*6
  r2 r4 f'8\ppp d'
  e' r8 r4 \tuplet 3/2 { f'8\mp e' d' } r4
  b16\ff b b8 r8 f'16\f e' ds'8 e'4 r8
  r2 f'8\mp e' ds' c'
  r4 r8. f'32\f e' \tuplet 3/2 { ds'4 e'8~ } e' g'16 gf'
  f'4. e'8 \tuplet 3/2 { e'16( f' e'~ } e'4.
  e'4) r4 f'16 e' d' cs' d' e' f' g'
  a'4 r4 r2 
  r4 r8 \tuplet 3/2 { d'16 f' a' } \tuplet 3/2 { b'4 a'8~ } a'8 f'16 d'
  e'4 r4 r4 \tuplet 6/4 { b'16\f a' g' f' e' d' }
  e'8 e'16 r8 f'16 r8 fs'16 r8 g'16 r8 gs'
  R1
  r2 r4 d'16\f r16 r8
  R1*2
  R1
  r2 r4 d'16\mf r16 r8
  R1*2
  a'4.\f gs' \tuplet 3/2 { g'16 a' g'~ } g'8
  f'4. e' d'16 e' f'8
  e'-> r4 f'8-> r4 e'8-> r8
  e'-> r4 f'8-> r4 e'8-> r8
  a'4.\mf gs' \tuplet 3/2 { g'16 a' g'~ } g'8
  f'4. e' d'16 e' f'8
  e'-> r4 f'8-> r4 e'8-> r8
  e'-> r4 f'8-> r4 e'8-> r8
  f'8\mf d'16\ff r f'8\mf r f' d'16\ff g' r f' e' ds'
  d'8\mf r d' r d' r d' \tuplet 3/2 { b16\mf b b }
  b8 r b b16\ff c' b8 r b\mf \tuplet 3/2 { b16 b b }
  b8 r b r b r b \tuplet 3/2 { f'16 f' f' }
  f'8 r f' r f' r f' \tuplet 3/2 { f'16 f' f' }
  f'8 r f' r f' r f' \tuplet 3/2 { b16 b b }
  b8 r b r b r \tuplet 6/4 { d'16\ff f' e' d' e' f' }
  b8\mf r b r b r b r
  
  e'8\mp d'32( e' d'8.) e'8 d'32( e' d'4..)
  e'8 d'32( e' d'8.) e'8 d'32( e' d'8.) e'8 d'
  f'8 e'32( f' e'8.) f'8 e'32( f' e'4..)
  f'8 e'32( f' e'8.) f'8 e'32( f' e'8.) r4
  R1*7
  r4. f'16\f( e') \tuplet 3/2 { ds'4 e'8~ } e' r8 
  R1
  r4. f'16\f( e') \tuplet 3/2 { ds'4 e'8~ } e' g'16 gf'
  f'4. e'8 e'2~
  e'4 r f'16( e') d' cs' d' e' f' g' 
  a'4 r8 d'16 cs' d'8 r4.
  d'4 r8 \tuplet 3/2 { d'16 r8 } r4. f'16 d'
  b8\p r r b r4 \tuplet 6/4 { r4 e'16\f d' }
  e'4 r2.
  g'8\mp f'32( g' f'8.) g'8 f'32( g' f'4..)
  g'8 f'32( g' f'8.) g'8 f'32( g' f'8.) e'8 d'
  f'8 e'32( f' e'8.) f'8 e'32( f' e'4..)
  f'8 e'32( f' e'8.) f'8 e'32( f' e'8.) r4
  R1*3
}

stringThree = {
  \global
  R1 
  r4 r8 a16(\ppp c') f'8 e' ds' c' 
  b c'16( b) a8 gs r2
  r4 r8 a16(\ppp c') f'8 e' ds' c' 
  b c'16( b) a8 gs r4 a16\ppp b c' cs'
  d'8 r8 r4 r2
  r2 d'8\ppp a r4
  r8 b16 c' b8 a16 b a8 gs16 a gs8 r8
  gs16\ff gs gs8 r4 r2
  R1
  b8\mp c'16( b) a8 gs r2
  r2 f'8\mp e' ds' c'
  b8\mp c'16( b) a8 gs r4 a16 b c' cs'
  d'8 r8 r8 d'16\f cs' d'8 a bf16 b c' cs'
  d'4 r d'8\mp a f' d'
  e' b16 c' b8 a16 b a8 gs16 a gs8 r8
  r16 bf8\f r16 cf'8 r16 c'8 r16 df'8 r16 d'8.
  R1*2
  r8 b\mp gs r b gs r b
  r8 b gs r b gs r b
  R1*2
  r8 b\p gs r b gs r b
  r8 b gs r b gs r b
  R1*2
  r8 b\mp gs r b gs r b
  r8 b gs r b gs r b
  R1*2
  r8 b\p gs r b gs r b
  r8 b gs r b gs r b
  d'8\mf r d' r d' r d' \tuplet 3/2 { d'16 d' d' }
  a16\ff cs' c' a a8\mf r a r a \tuplet 3/2 { gs16 gs gs } 
  gs8 r gs r gs r gs \tuplet 3/2 { gs16 gs gs }
  gs8 r gs r gs r gs \tuplet 3/2 { d'16 d' d' }
  d'8 r d' r d' r d' \tuplet 3/2 { d'16 d' d' }
  d'8 r d' r d' r d' \tuplet 3/2 { gs16 gs gs }
  gs8 r gs r \tuplet 6/4 { gs16\ff a b gs a b } b8\mf \tuplet 3/2 { b16 b b }
  gs8 r gs r gs r gs r
  R1
  r2 r4 c'8\mp b
  d'8 c'32( d' c'8.) d'8 c'32( d' c'4..)
  d'8 c'32( d' c'8.) d'8 c'32( d' c'8.) d'8 c'
  e'8 ds'32( e' ds'8.) e'8 ds'32( e' ds'4..)
  e'8 d'32( e' d'8.) e'8 d'32( e' d'8~ d'32) e'( f'8) e'
  d' c' r d' c'4 b8 a
  b32 a gs8.~ gs2.
  R1*8
  r2 r8 a\f bf16 b c' cs'
  R1
  gs8\p r r gs r2
  r16 bf8\mp r16 cf'8 r16 c'8 r16 df'8 r16 d'8.
  e'8\mp d'32( e' d'8.) e'8 d'32( e' d'4..)
  e'8 d'32( e' d'8.) e'8 d'32( e' d'8.) e'8 d'
  d'8 c'32( d' c'8.) d'8 c'32( d' c'4..)
  d'8 c'32( d' c'8.) d'8 c'32( d' c'8.) d'8 c'
  e'8 ds'32( e' ds'8.) e'8 ds'32( e' ds'4..)
  e'8 d'32( e' d'8.) e'8 d'32( e' d'8~ d'32) e'( f'8) e'
  d' c' r d' c'4 b8 a
}

stringFour = {
  \global
  r4 r8 f16(\mp e ds8) e4 a16 b c'1~ 
  c'4 r8 f16(\mp e ds8) e4 g16 gf
  f4. e8 \tuplet 3/2 { e16( f e~ } e4.
  e4) r4 a16(\ppp gs) f gs r4
  r8 a f e r2
  r4 r8 \tuplet 3/2 { d16\mp f a } b8. a32( b a8) f16 d 
  e4. r8 r4 \tuplet 3/2 { b8 gs f }
  e16\ff e e8 r4 r2
  f8\mp r8 e a16( c'16) r2 
  r2 a16( g) f8 e r8
  f8\mp r8 e a16( c'16) r2 
  R1
  r8 a8\mp f e r2
  r4 a8\mp d r2
  R1
  e8.->\f f-> fs-> g-> gs4->
  f8\mp-> e a gs-> f e f-> r8
  f-> e a g-> f e f-> r8
  f-> e gs f-> e d f-> e
  f-> e gs f-> e d f16-> r16 r8
  f8\p-> e a gs-> f e f-> r8
  f-> e a g-> f e f-> r8
  f-> e gs f-> e d f-> e
  f-> e gs f-> e d f16-> r16 r8
  R1*8
  a8\mf r a r a r a \tuplet 3/2 { a16 a a }
  a8 r g16\ff f e ef d4 r8 e16 f
  e8\mf gs16\ff a e8\mf r e r e \tuplet 3/2 { e16 e e }
  e8 r e r e r e \tuplet 3/2 { a16 a a }
  a8 r a r a r a \tuplet 3/2 { a16 a a }  
  a8 r a r a r a \tuplet 3/2 { e16 e e }  
  e8 r \tuplet 6/4 { d16\ff f e d e f } gs8\mf r gs \tuplet 3/2 { gs16 gs gs }
  e8 r e r e r e r
  
  R1*3
  r2 r4 b8\mp a
  c'8 b32( c' b8.) c'8 b32( c' b4..)
  c'8 b32( c' b8.) c'8 b32( c' b8~ b32) b( d'8) c'
  b a r b a4 g32 a g16 f8
  f8\pp->\< \xNote { f f } f-> \xNote { f f } f-> \xNote { f }
  f-> \xNote { f f } f-> \xNote { f f } f-> \xNote { f }
  f-> \xNote { f f } f-> \xNote { f f } f-> \xNote { f }
  f-> \xNote { f f } f-> \xNote { f f } f-> \tuplet 3/2 { f16 g f }
  f8\ff r2..
  f8->\p e r f-> e r f-> e
  f8-> e r f-> e r f-> e
  f8-> e r f-> e r f-> e
  f8-> e r f-> e r f-> e
  d8-> d r e-> e r e-> e
  d8-> d r e-> e r e-> gs
  e e r e e r e e
  e8.->\f f-> fs-> g-> gs4->
  R1*3
  r2 r4 b8\mp a
  c'8 b32( c' b8.) c'8 b32( c' b4..)
  c'8 b32( c' b8.) c'8 b32( c' b8~ b32) b( d'8) c'
  b a r b a4 g32 a g16 f8
}

stringFive = {
  \global
  R1 
  f8\mp r8 e8\ppp r r2
  r2 a16( gs) f8 e c
  f4\mp e8\ppp r8 r2
  r4 r8 \tuplet 3/2 { a,16\mp c e } f8 e4 f16 g
  a4. d16 cs d2~
  d4 a8\ppp d r2
  r2 r4 r8 e16 f
  b,16\ff b, b,8 r4 r2
  R1*3
  r2 a16( g) f g r4
  r2 d8\mp a, bf,16 b, c cs
  d8 a, r4 r2
  R1*2
  r2 r4 r8 c\mp
  r2 r4 r8 c
  R1
  r2 r4 r16 c b, b,
  r2 r4 r8 c\p
  r2 r4 r8 c
  R1
  r2 r4 r16 c b, b,
  R1*8
  d8\mf r d r d r d \tuplet 3/2 { d16 d d }
  d8 r d r d r d \tuplet 3/2 { b,16 b, b, }
  b,8 r b, r b, r b, \tuplet 3/2 { b,16 b, b, }
  b,8 r b, r b, r b, \tuplet 3/2 { d16 d d }
  d8 r d r d r d \tuplet 3/2 { d16 d d }
  d8 r d r d r d \tuplet 3/2 { b,16 b, b, }
  b,8 \tuplet 3/2 { r16 a,\ff b, } b,8\mf r b, r b, \tuplet 3/2 { b,16 b, b, }
  b,8 r b, r b, r b, r
  R1*7
  b,8\pp->\< \xNote { b, b, } b,-> \xNote { b, b, } b,-> \xNote { b, }
  b,-> \xNote { b, b, } b,-> \xNote { b, b, } b,-> \xNote { b, }
  b,-> \xNote { b, b, } b,-> \xNote { b, b, } b,-> \xNote { b, }
  b,-> \xNote { b, b, } b,-> \xNote { b, b, } b,-> r
  b,\ff r2..
  a,8\p-> a, r a,-> a, r a,-> a,
  a,8-> a, r a,-> a, r a,-> a,
  a,8-> a, r a,-> a, r a,-> a,
  a,8-> a, r a,-> a, r a,-> a,
  a,8-> a, r a,-> a, r a,-> a,
  a,8-> a, r a,-> a, r a,-> r
  b, b, r b, b, r b, b,
  R1*8
}

stringSix = {
  \global
  R1 
  a,8\mp r8 r4 r2
  R1
  a,4\mp r4 r2
  R1 
  r2 d8\ppp a, bf,16 b, c cs
  d8 a, r4 r2
  R1
  e,16\ff e, e,8 r4 r2
  a,8\mp r8 r4 r2
  r2 r4 r8 c
  a, r8 r4 r2
  R1*22
  r2 r4 r8 \tuplet 3/2 { e,16\mf e, e, }
  e,8 r e, r e, r e, \tuplet 3/2 { e,16 e, e, }
  e,8 r e, r e, r e, r
  R1
  r2 r4 r8 \tuplet 3/2 { e,16\f e, e, }
  e,8 \tuplet 3/2 { gs,16\ff r r } e,8\mf r e, r e, \tuplet 3/2 { e,16 e, e, }
  e,8 r e, r e, r e, r
  
  R1*7
  e,8\pp->\< \xNote { e, e, } e,-> \xNote { e, e, } e,-> \xNote { e, }
  e,-> \xNote { e, e, } e,-> \xNote { e, e, } e,-> \xNote { e, }
  e,-> \xNote { e, e, } e,-> \xNote { e, e, } e,-> \xNote { e, }
  e,-> \xNote { e, e, } e,-> \xNote { e, e, } e,-> r
  e,\ff r2..
  e,8\p-> e, r e,-> e, r e,-> e,
  e,8-> e, r e,-> e, r e,-> e,
  e,8-> e, r e,-> e, r e,-> e,
  e,8-> e, r e,-> e, r e,-> e,
  R1
  r2.. e,8
  e, e, r e, e, r e, e,
  R1*8
}


\score {
  <<
    % High E
    \new Staff \with {
      instrumentName = "E4"
    } {
      \clef "treble_8"
      \stringOne
    }

    % B
    \new Staff \with {
      instrumentName = "B3"
    } {
      \clef "treble_8"
      \stringTwo
    }

    % G
    \new Staff \with {
      instrumentName = "G3"
    } {
      \clef "treble_8"
      \stringThree
    }

    % D
    \new Staff \with {
      instrumentName = "D3"
    } {
      \clef "treble_8"
      \stringFour
    }

    % A
    \new Staff \with {
      instrumentName = "A2"
    } {
      \clef "treble_8"
      \stringFive
    }

    % Low E
    \new Staff \with {
      instrumentName = "E2"
    } {
      \clef "treble_8"
      \stringSix
    }
  >>

  \layout { }
}