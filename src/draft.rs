use std::collections::HashMap;
use std::rc::Rc;
use std::str;
use std::fmt;
use std::iter;
use std::hash;
use num::BigUint;
use digit_slice::{DigitSlice, FromDigits};

/// A wrapper for referencing Noun-like patterns.
#[derive(Copy, Clone)]
pub enum Shape<A, N> {
    Atom(A),
    Cell(N, N),
}

/// A Nock noun, the basic unit of representation.
///
/// A noun is an atom or a cell. An atom is any natural number. A cell is any
/// ordered pair of nouns.
///
/// Atoms are represented by a little-endian byte array of 8-bit digits.
#[derive(Clone, PartialEq, Eq)]
pub struct Noun(Inner);

#[derive(Clone, PartialEq, Eq)]
enum Inner {
    Atom(Rc<Vec<u8>>),
    Cell(Rc<Noun>, Rc<Noun>),
}

pub type NounShape<'a> = Shape<&'a [u8], &'a Noun>;

impl Noun {
    fn get<'a>(&'a self) -> NounShape<'a> {
        match self.0 {
            Inner::Atom(ref v) => Shape::Atom(&v),
            Inner::Cell(ref a, ref b) => Shape::Cell(&*a, &*b),
        }
    }

    /// Pattern-match a noun with shape [p q r].
    ///
    /// The digit sequence shows the branch length of each leaf node in the
    /// expression being matched. 122 has the leftmost leaf 1 step away from
    /// the root and the two leaves on the right both 2 steps away from the
    /// root.
    pub fn get_122<'a>
        (&'a self)
         -> Option<(NounShape<'a>, NounShape<'a>, NounShape<'a>)> {
        if let Shape::Cell(ref a, ref b) = self.get() {
            if let Shape::Cell(ref b, ref c) = b.get() {
                return Some((a.get(), b.get(), c.get()));
            }
        }
        None
    }

    /// Pattern-match a noun with shape [[p q] r].
    pub fn get_221<'a>
        (&'a self)
         -> Option<(NounShape<'a>, NounShape<'a>, NounShape<'a>)> {
        if let Shape::Cell(ref a, ref c) = self.get() {
            if let Shape::Cell(ref a, ref b) = a.get() {
                return Some((a.get(), b.get(), c.get()));
            }
        }
        None
    }

    /// Memory address or other unique identifier for the noun.
    fn addr(&self) -> usize {
        &*self as *const _ as usize
    }

    /// Build a new atom noun from a little-endian 8-bit digit sequence.
    pub fn atom(digits: &[u8]) -> Noun {
        Noun(Inner::Atom(Rc::new(digits.to_vec())))
    }

    /// Build a new cell noun from two existing nouns.
    pub fn cell(a: Noun, b: Noun) -> Noun {
        Noun(Inner::Cell(Rc::new(a), Rc::new(b)))
    }

    /// Build a noun from a convertible value.
    pub fn from<T: ToNoun>(item: T) -> Noun {
        item.to_noun()
    }

    /// Match noun if it's an atom that's a small integer.
    ///
    /// Will not match atoms that are larger than 2^32, but is not guaranteed
    /// to match atoms that are smaller than 2^32 but not by much.
    pub fn as_u32(&self) -> Option<u32> {
        if let &Noun(Inner::Atom(ref digits)) = self {
            u32::from_digits(digits).ok()
        } else {
            None
        }
    }

    /// Run a memoizing fold over the noun
    fn fold<'a, F, T>(&'a self, mut f: F) -> T
        where F: FnMut(Shape<&'a [u8], T>) -> T,
              T: Clone
    {
        fn h<'a, F, T>(noun: &'a Noun,
                       memo: &mut HashMap<usize, T>,
                       f: &mut F)
                       -> T
            where F: FnMut(Shape<&'a [u8], T>) -> T,
                  T: Clone
        {
            let key = noun.addr();

            if memo.contains_key(&key) {
                memo.get(&key).unwrap().clone()
            } else {
                let ret = match noun.get() {
                    Shape::Atom(x) => f(Shape::Atom(x)),
                    Shape::Cell(ref a, ref b) => {
                        let a = h(*a, memo, f);
                        let b = h(*b, memo, f);
                        let ret = f(Shape::Cell(a, b));
                        ret
                    }
                };
                memo.insert(key, ret.clone());
                ret
            }
        }

        h(self, &mut HashMap::new(), &mut f)
    }
}

impl hash::Hash for Noun {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        fn f<H: hash::Hasher>(state: &mut H, shape: Shape<&[u8], u64>) -> u64 {
            match shape {
                Shape::Atom(x) => x.hash(state),
                Shape::Cell(a, b) => {
                    a.hash(state);
                    b.hash(state);
                }
            }
            state.finish()
        }
        self.fold(|x| f(state, x))
            .hash(state);
    }
}

impl iter::FromIterator<Noun> for Noun {
    fn from_iter<T>(iterator: T) -> Self
        where T: IntoIterator<Item = Noun>
    {
        let mut v: Vec<Noun> = iterator.into_iter().collect();
        v.reverse();

        v.into_iter()
         .fold(None, move |acc, i| {
             acc.map_or_else(|| Some(i.clone()),
                             |a| Some(Noun::cell(i.clone(), a)))
         })
         .expect("Can't make noun from empty list")
    }
}



/// Trait for types that can convert themselves to a noun.
pub trait ToNoun {
    fn to_noun(&self) -> Noun;
}

impl<T> ToNoun for T where T: DigitSlice
{
    fn to_noun(&self) -> Noun {
        Noun::atom(self.as_digits())
    }
}


/// A trait for types that can be instantiated from a Nock noun.
pub trait FromNoun: Sized {
    /// The associated error.
    type Err;

    /// Try to convert a noun to an instance of the type.
    fn from_noun(n: &Noun) -> Result<Self, Self::Err>;
}

impl<T> FromNoun for T where T: FromDigits
{
    type Err = ();

    fn from_noun(n: &Noun) -> Result<Self, Self::Err> {
        match n.get() {
            Shape::Atom(x) => T::from_digits(x).map_err(|_| ()),
            _ => Err(()),
        }
    }
}

impl<T, U> FromNoun for (T, U)
    where T: FromNoun,
          U: FromNoun
{
    type Err = ();

    fn from_noun(n: &Noun) -> Result<Self, Self::Err> {
        match n.get() {
            Shape::Cell(a, b) => {
                let t = try!(T::from_noun(a).map_err(|_| ()));
                let u = try!(U::from_noun(b).map_err(|_| ()));
                Ok((t, u))
            }
            _ => Err(()),
        }
    }
}

// TODO: FromNoun for T: FromIterator<U: FromNoun>. Pair impl should give us
// a HashMap derivation then. Use ~-terminated cell sequence as backend.

// TODO: Turn a ~-terminated noun into a vec or an iter. Can fail if the last
// element isn't a ~, and we'll only know when we hit the last element...
// Return type is Option<Vec<&'a Noun>>?

// TODO: ToNoun for T: IntoIterator<U: ToNoun>.

// TODO: FromNoun/ToNoun for String, compatible with cord datatype.

// TODO: FromNoun/ToNoun for signed numbers using the Urbit representation
// convention.


// Into-conversion is only used so that we can put untyped numeric literals in
// the noun-constructing macro and have them typed as unsigned. If the noun
// constructor uses ToNoun, literals are assumed to be i32, which does not map
// to atoms in quite the way we want.
impl Into<Noun> for u64 {
    fn into(self) -> Noun {
        Noun::from(self)
    }
}

/// Macro for noun literals.
///
/// Rust n![1, 2, 3] corresponds to Nock [1 2 3]
#[macro_export]
macro_rules! n {
    [$x:expr, $y:expr] => { $crate::draft::Noun::cell($x.into(), $y.into()) };
    [$x:expr, $y:expr, $($ys:expr),+] => { $crate::draft::Noun::cell($x.into(), n![$y, $($ys),+]) };
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct NockError;

pub type NockResult = Result<Noun, NockError>;

/// Evaluate the nock `*[subject formula]`
pub fn nock_on(mut subject: Noun, mut formula: Noun) -> NockResult {
    use std::u32;

    loop {
        if let Shape::Cell(ops, tail) = formula.clone().get() {
            match ops.as_u32() {
                // Axis
                Some(0) => {
                    match tail.get() {
                        Shape::Atom(ref x) => return axis(x, &subject),
                        _ => return Err(NockError),
                    }
                }
                // Just
                Some(1) => return Ok(tail.clone()),
                // Fire
                Some(2) => {
                    match tail.get() {
                        Shape::Cell(ref b, ref c) => {
                            let p = try!(nock_on(subject.clone(),
                                                 (*b).clone()));
                            let q = try!(nock_on(subject, (*c).clone()));
                            subject = p;
                            formula = q;
                            continue;
                        }
                        _ => return Err(NockError),
                    }
                }
                // Depth
                Some(3) => {
                    let p = try!(nock_on(subject.clone(), (*tail).clone()));
                    return match p.get() {
                        Shape::Cell(_, _) => Ok(Noun::from(0u32)),
                        _ => Ok(Noun::from(1u32)),
                    };
                }
                /*
                // Bump
                4 => {
                    let p = try!(tar(Cell(subject, tail)));
                    return match *p {
                        // Switch to BigAtoms at regular atom size limit.
                        Atom(u32::MAX) => {
                            Ok(Rc::new(BigAtom(BigUint::from_u32(u32::MAX).unwrap() +
                                               BigUint::one())))
                        }
                        Atom(ref x) => Ok(Rc::new(Atom(x + 1))),
                        BigAtom(ref x) => Ok(Rc::new(BigAtom(x + BigUint::one()))),
                        _ => Err(NockError),
                    };
                }
                // Same
                5 => {
                    let p = try!(tar(Cell(subject, tail)));
                    return match *p {
                        Cell(ref a, ref b) => {
                            if a == b {
                                return Ok(Rc::new(Atom(0)));
                            } else {
                                return Ok(Rc::new(Atom(1)));
                            }
                        }
                        _ => return Err(NockError),
                    };
                }

                // If
                6 => {
                    if let Some((b, c, d)) = tail.as_triple() {
                        let p = try!(tar(Cell(subject.clone(), b)));
                        match *p {
                            Atom(0) => noun = Cell(subject, c),
                            Atom(1) => noun = Cell(subject, d),
                            _ => return Err(NockError),
                        }
                        continue;
                    } else {
                        return Err(NockError);
                    }
                }

                // Compose
                7 => {
                    match *tail {
                        Cell(ref b, ref c) => {
                            let p = try!(tar(Cell(subject, b.clone())));
                            noun = Cell(p, c.clone());
                            continue;
                        }
                        _ => return Err(NockError),
                    }
                }

                // Push
                8 => {
                    match *tail {
                        Cell(ref b, ref c) => {
                            let p = try!(tar(Cell(subject.clone(), b.clone())));
                            noun = Cell(Rc::new(Cell(p, subject)), c.clone());
                            continue;
                        }
                        _ => return Err(NockError),
                    }
                }

                // Call
                9 => {
                    match *tail {
                        Cell(ref b, ref c) => {
                            let p = try!(tar(Cell(subject.clone(), c.clone())));
                            let q = try!(tar(Cell(p.clone(),
                                                  Rc::new(Cell(Rc::new(Atom(0)),
                                                               b.clone())))));
                            noun = Cell(p, q);
                            continue;
                        }
                        _ => return Err(NockError),
                    }
                }

                // Hint
                10 => {
                    match *tail {
                        Cell(ref _b, ref c) => {
                            // Throw away b.
                            // XXX: Should check if b is a cell and fail if it
                            // would crash.
                            noun = Cell(subject, c.clone());
                            continue;
                        }
                        _ => return Err(NockError),
                    }
                }
                */
                None => {
                    if let Shape::Cell(n, tail) = ops.get() {
                        // Autocons
                        let a = try!(nock_on(subject.clone(), n.clone()));
                        let b = try!(nock_on(subject, tail.clone()));
                        return Ok(Noun::cell(a, b));
                    } else {
                        return Err(NockError);
                    }
                }

                _ => return Err(NockError),
            }
        } else {
            return Err(NockError);
        }
    }
}

fn axis(atom: &[u8], subject: &Noun) -> NockResult {
    unimplemented!();
}


#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct ParseError;

impl str::FromStr for Noun {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, ParseError> {
        return parse(&mut s.chars().peekable());

        fn parse<I: Iterator<Item = char>>(input: &mut iter::Peekable<I>)
                                           -> Result<Noun, ParseError> {
            eat_space(input);
            match input.peek().map(|&x| x) {
                Some(c) if c.is_digit(10) => parse_atom(input),
                Some(c) if c == '[' => parse_cell(input),
                _ => Err(ParseError),
            }
        }

        /// Parse an atom, a positive integer.
        fn parse_atom<I: Iterator<Item = char>>(input: &mut iter::Peekable<I>)
                                                -> Result<Noun, ParseError> {
            let mut buf = Vec::new();

            loop {
                if let Some(&c) = input.peek() {
                    if c.is_digit(10) {
                        input.next();
                        buf.push(c);
                    } else if c == '.' {
                        // Dot is used as a sequence separator (*not* as
                        // decimal point). It can show up anywhere in the
                        // digit sequence and will be ignored.
                        input.next();
                    } else if c == '[' || c == ']' || c.is_whitespace() {
                        // Whitespace or cell brackets can terminate the
                        // digit sequence.
                        break;
                    } else {
                        // Anything else in the middle of the digit sequence
                        // is an error.
                        return Err(ParseError);
                    }
                } else {
                    break;
                }
            }

            if buf.len() == 0 {
                return Err(ParseError);
            }

            let num: BigUint = buf.into_iter()
                                  .collect::<String>()
                                  .parse()
                                  .expect("Failed to parse atom");

            Ok(Noun::from(num))
        }

        /// Parse a cell, a bracketed pair of nouns.
        ///
        /// For additional complication, cells can have the form [a b c] which
        /// parses to [a [b c]].
        fn parse_cell<I: Iterator<Item = char>>(input: &mut iter::Peekable<I>)
                                                -> Result<Noun, ParseError> {
            let mut elts = Vec::new();

            if input.next() != Some('[') {
                panic!("Bad cell start");
            }

            // A cell must have at least two nouns in it.
            elts.push(try!(parse(input)));
            elts.push(try!(parse(input)));

            // It can have further trailing nouns.
            loop {
                eat_space(input);
                match input.peek().map(|&x| x) {
                    Some(c) if c.is_digit(10) => {
                        elts.push(try!(parse_atom(input)))
                    }
                    Some(c) if c == '[' => elts.push(try!(parse_cell(input))),
                    Some(c) if c == ']' => {
                        input.next();
                        break;
                    }
                    _ => return Err(ParseError),
                }
            }



            Ok(elts.into_iter().collect())
        }

        fn eat_space<I: Iterator<Item = char>>(input: &mut iter::Peekable<I>) {
            loop {
                match input.peek().map(|&x| x) {
                    Some(c) if c.is_whitespace() => {
                        input.next();
                    }
                    _ => return,
                }
            }
        }
    }
}

impl fmt::Display for Noun {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.0 {
            Inner::Atom(ref n) => return dot_separators(f, &n),
            Inner::Cell(ref a, ref b) => {
                try!(write!(f, "[{} ", a));
                // List pretty-printer.
                let mut cur = b;
                loop {
                    match cur.0 {
                        Inner::Cell(ref a, ref b) => {
                            try!(write!(f, "{} ", a));
                            cur = &b;
                        }
                        Inner::Atom(ref n) => {
                            try!(dot_separators(f, &n));
                            return write!(f, "]");
                        }
                    }
                }
            }
        }

        fn dot_separators(f: &mut fmt::Formatter,
                          digits: &[u8])
                          -> fmt::Result {
            let s = format!("{}", BigUint::from_digits(digits).unwrap());
            let phase = s.len() % 3;
            for (i, c) in s.chars().enumerate() {
                if i > 0 && i % 3 == phase {
                    try!(write!(f, "."));
                }
                try!(write!(f, "{}", c));
            }
            Ok(())
        }
    }
}

impl fmt::Debug for Noun {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}


#[cfg(test)]
mod tests {
    use std::hash;
    use super::Noun;
    use num::BigUint;

    fn parses(input: &str, output: Noun) {
        assert_eq!(input.parse::<Noun>().ok().expect("Parsing failed"), output);
    }

    #[test]
    fn scratch() {
        let x = Noun::from(123u32);
        assert!(x == Noun::from(123u8));
    }

    fn hash<T: hash::Hash>(t: &T) -> u64 {
        use std::hash::Hasher;
        let mut s = hash::SipHasher::new();
        t.hash(&mut s);
        s.finish()
    }

    #[test]
    fn test_fold() {
        assert_eq!(hash(&n![1, 2, 3]), hash(&n![1, 2, 3]));
        assert!(hash(&n![n![1, 2], 3]) != hash(&n![1, 2, 3]));
        assert!(hash(&n![1, 2, 3]) != hash(&n![1, 2]));
    }

    #[test]
    fn test_parser() {
        use num::traits::Num;

        assert!("".parse::<Noun>().is_err());
        assert!("12ab".parse::<Noun>().is_err());
        assert!("[]".parse::<Noun>().is_err());
        assert!("[1]".parse::<Noun>().is_err());

        parses("0", Noun::from(0u32));
        parses("1", Noun::from(1u32));
        parses("1.000.000", Noun::from(1_000_000u32));

        parses("4294967295", Noun::from(4294967295u32));
        parses("4294967296", Noun::from(4294967296u64));

        parses("999.999.999.999.999.999.999.999.999.999.999.999.999.999.999.\
                999.999.999.999.999",
               Noun::from(BigUint::from_str_radix("999999999999999999999999\
                                                   999999999999999999999999\
                                                   999999999999",
                                                  10)
                              .unwrap()));

        parses("[1 2]", n![1, 2]);
        parses("[1 2 3]", n![1, 2, 3]);
        parses("[1 [2 3]]", n![1, 2, 3]);
        parses("[[1 2] 3]", n![n![1, 2], 3]);
    }
}
