using System;
using System.Collections.Generic;
using System.Globalization;

namespace Taba
{
    /// <summary>A <c>PATCH</c> field that accepts going back to the server default (<c>null</c>).</summary>
    public struct Field<T> where T : struct, IFormattable
    {
        private readonly int _state;
        private readonly T _value;

        private Field(int state, T value)
        {
            _state = state;
            _value = value;
        }

        /// <summary>Does not send the field (the initial value).</summary>
        public static Field<T> Keep
        {
            get { return default(Field<T>); }
        }

        /// <summary>Sends <c>null</c>: goes back to the server default.</summary>
        public static Field<T> UseDefault
        {
            get { return new Field<T>(1, default(T)); }
        }

        public static Field<T> Set(T value)
        {
            return new Field<T>(2, value);
        }

        public static implicit operator Field<T>(T value)
        {
            return Set(value);
        }

        internal void Push(string name, List<string> fields)
        {
            if (_state == 1)
            {
                fields.Add("\"" + name + "\":null");
            }
            else if (_state == 2)
            {
                fields.Add("\"" + name + "\":" + _value.ToString(null, CultureInfo.InvariantCulture));
            }
        }
    }
}
