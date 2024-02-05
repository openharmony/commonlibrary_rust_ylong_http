// Copyright (c) 2023 Huawei Device Co., Ltd.
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! HTTP Legitimate Header characters.

// token          = 1*tchar
// tchar          = "!" / "#" / "$" / "%" / "&" / "'" / "*"
//                  / "+" / "-" / "." / "^" / "_" / "`" / "|" / "~"
//                  / DIGIT / ALPHA
//                  ; any VCHAR, except delimiters
// delimitersd    = DQUOTE and "(),/:;<=>?@[\]{}"
#[rustfmt::skip]
pub(crate) static HEADER_NAME_BYTES: [bool; 256] = {
    const __: bool = false;
    const TT: bool = true;
    [
//      \0                                  HT  LF          CR
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __,
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __,
//      \w  !   "   #   $   %   &   '   (   )   *   +   ,   -   .   /
        __, TT, __, TT, TT, TT, TT, TT, __, __, TT, TT, __, TT, TT, __,
//      0   1   2   3   4   5   6   7   8   9   :   ;   <   =   >   ?
        TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, __, __, __, __, __, __,
//      @   A   B   C   D   E   F   G   H   I   J   K   L   M   N   O
        __, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT,
//      P   Q   R   S   T   U   V   W   X   Y   Z   [   \   ]   ^   _
        TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, __, __, __, TT, TT,
//      `   a   b   c   d   e   f   g   h   i   j   k   l   m   n   o
        TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT,
//      p   q   r   s   t   u   v   w   x   y   z   {   |   }   ~   del
        TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, __, TT, __, TT, __,
// Expand ascii
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __,
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __,
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __,
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __,
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __,
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __,
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __,
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __,
    ]
};

// field-value    = *( field-content / obs-fold )
// field-content  = field-vchar [ 1*( SP / HTAB ) field-vchar ]
// field-vchar    = VCHAR / obs-text
//
// obs-fold       = CRLF 1*( SP / HTAB )
//                  ; obsolete line folding
//                  ; see Section 3.2.4
#[rustfmt::skip]
pub(crate) static HEADER_VALUE_BYTES: [bool; 256] = {
    const __: bool = false;
    const TT: bool = true;
    [
//      \0                                  HT  LF          CR
        __, __, __, __, __, __, __, __, __, TT, __, __, __, __, __, __,
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __,
//      \w  !   "   #   $   %   &   '   (   )   *   +   ,   -   .   /
        TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT,
//       0   1   2   3   4   5   6   7   8   9   :   ;   <   =   >   ?
        TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT,
//       @   A   B   C   D   E   F   G   H   I   J   K   L   M   N   O
        TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT,
//       P   Q   R   S   T   U   V   W   X   Y   Z   [   \   ]   ^   _
        TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT,
//       `   a   b   c   d   e   f   g   h   i   j   k   l   m   n   o
        TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT,
//       p   q   r   s   t   u   v   w   x   y   z   {   |   }   ~   del
        TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, __,
// Expand ascii
        TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT,
        TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT,
        TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT,
        TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT,
        TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT,
        TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT,
        TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT,
        TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT,
    ]
};
